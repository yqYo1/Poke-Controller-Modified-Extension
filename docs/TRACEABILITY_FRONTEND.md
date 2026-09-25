# フロントエンド要求トレーサビリティ（直接証拠）

> 対象仕様: `docs/SPECIFICATION_FRONTEND.md`（本ファイル作成時点の worktree 内容）
> 検証基準: HEAD `82b973e187e0fac0a26abf65541834e4c6ac72d4` + dirty/staged 変更（`web/src` に実装変更あり。判定は現行ソースとテストを対象にする）。
> 判定区分: **実装済**（コードに直接存在）/ **部分的**（一部のみ・仕様との差あり）/
> **未実装**（コードに存在しない）/ **将来**（仕様が未実装と明示）/ **対象外**（仕様が backend 所有・他書所有・実装しないと明示）。
> 実装の修正は行わない。本書は証拠とギャップの記録のみである。

## 0. 分類と frontend 選択

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| Svelte Web UI / Tauri desktop UI を維持する | 実装済 | `web/src/`（SvelteKit 一式）; `rust/pokecon/tauri.conf.json`; `flake.nix` の `tauri-build` ジョブ | — |
| GPUI native UI を同一 Rust backend へ追加し `nix run` から選択起動 | 将来 | 仕様が「未実装」と明示。`flake.nix` に `.#gpui` エントリなし、`--ui web`（`flake.nix` 約3255行）のみ | GPUI 入口は実装後に受入 |
| WASM 版 GPUI は要件に含めない | 対象外 | 仕様の明示的除外。コード側の対応物なし（正しい） | — |
| `--ui` 選択は process 起動時に固定、既存 process の移行なし | 対象外 | backend/CLI の責務。frontend 側に対応物なし（正しい） | — |
| 現行 Web/Tauri 実装技術を GPUI へ強制しない | 対象外 | 方針宣言。コード側の対応物なし（正しい） | — |

### 0.1 必須要件

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| GPUI 受入・旧 UI 残存判断まで現行 Svelte Web/Tauri UI を削除しない | 実装済 | `web/src/` が現存し削除されていない | — |
| Web/Tauri/GPUI は同一 Rust backend を使い hardware・canonical state を重複所有しない | 対象外 | backend 所有。frontend は `ApplicationRuntime`（`web/src/lib/runtime.ts`）経由で backend 状態を参照するのみ | — |
| `nix run . -- --ui web` / `nix run .#tauri` / `nix run .#gpui` の選択目標を維持 | 部分的 | `--ui web`（`flake.nix` 約3255行）、`--ui desktop`（`flake.nix` 約5382行）、`tauri-build` ジョブは存在。`.#gpui` エントリなし | `.#gpui` は将来実装（仕様どおり） |

### 0.2 機能要件（所有分担）

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| §3.1–§3.3、§5、§6（一部除外）、§9、§13 は対象 frontend の操作・結果を規定 | — | 本書の各節で個別に検証 | 宣言文自体にコード対応物はない |
| 連携定義書 §3.4・6.1.5・6.2.2 は end-to-end 契約、backend 定義書 §6.1.6 は camera resource 契約が所有 | 対象外 | backend/連携所有。frontend は API 呼び出し側（`web/src/lib/actions.ts`） | — |
| GPUI では native 表示へ適合、Tkinter pixel 配置・widget 種別の複製を要求しない | 対象外 | 方針宣言 | — |
| `ui.*` 等の設定 ID・型・既定値・scope・保存・validation は backend・正準 registry が所有。frontend は独自 schema を定義しない | 実装済 | `OtherTab.svelte:6-14`（`SelectSetting`/`BooleanSetting` は backend ID の参照のみ）; 生成型 `web/src/lib/api/openapi.ts` を使用 | — |
| 設定値の表示・操作 UI は本書が所有 | 実装済 | `OtherTab.svelte`、`CameraTab.svelte`、`CommandsTab.svelte`、`NotificationsTab.svelte`、`ManualTab.svelte`、`SerialTab.svelte` | — |
| `ui.desktop.disable_compositing` は Tauri/WebView 専用、GPUI に表示・適用しない | 実装済 | `OtherTab.svelte` の compositing チェックボックスは Web モードで「値だけ保持し効果なし」と表示し、GPUI 分岐なし（GPUI 自体が未実装のため表示対象外） | — |
| Web browser 専用 version 要件・video transport 要件を native GPUI へ誤適用しない | 対象外 | 方針宣言 | — |

## 1.1 目的 / 1.4 対象外機能

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| 本節は各 frontend の操作・表示・状態遷移・利用者向け品質目標を規定（規範要件と実装状況を区別） | — | 宣言文。本書全体で検証 | — |
| キーボードショートカット（キーマップ）機能は実装しない。ショートカットボタン（10ボタン）は実装する | 実装済 | キーマップ割当 UI なし（`web/src` に `keymap` 0件）。10ボタンは `CommandsTab.svelte:12,27`（`ShortcutIndex = 1..10`、`shortcutIndexes`） | 正しく対象外化されている |

## 3. 非機能要件

### 3.1 パフォーマンス

| 指標 | 目標 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|------|----------|----------------|
| ビデオ遅延 WebRTC | < 100ms | 対象外（frontend に遅延表明なし） | transport 本体は backend 所有。frontend は `CameraViewport.svelte` で WebRTC track→canvas 描画、`media.ts` の `MediaTransport`/`MediaMode` | 遅延の計測・表明コードなし。仕様も Web 経路適用と明記 |
| ビデオ遅延 MJPEG+WebSocket フォールバック | 50–150ms | 対象外（同上） | `CameraViewport.svelte:190-204` `receiveFallback()`（MJPEG Blob→ObjectURL→canvas） | 同上 |
| コントローラー入力遅延 | < 50ms | 部分的 | `AnalogStick.svelte:20,40-46`（rAF 同期・16ms スロットル）、`CameraViewport.svelte:220,294`（スティック/script ポインタも 16ms） | 遅延の計測・表明コードなし。機構のみ存在 |
| UI 応答性 | 60FPS・<16ms 入力応答 | 部分的 | 同上 rAF 機構。FPS 選択は `CameraTab.svelte:281-282`（`ui.fps`/`ui.fps_options`） | フレームレート実測・表明コードなし |

### 3.2 アクセシビリティ

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| すべてのコントロールのキーボードナビゲーション | 部分的 | メインタブの tablist キー操作 `+page.svelte:53-70`（Arrow/Home/End + `handleTabKeydown`、`role=tablist/tab`、`aria-selected/controls`、`tabindex` 管理）; サブタブも `CommandsTab.svelte:239-258`（`subtabKeydown`）。`AnalogStick.svelte:79-109`（Arrow/Home キー操作+`preventDefault`） | タブ領域以外の全コントロール網羅ではない。コントローラーボタン自体はポインタ専用 |
| スクリーンリーダー用 ARIA ラベル | 実装済 | `+page.svelte:111,118,132,142-160`（`aria-hidden/live/label`、`role=status/tablist/tab/tabpanel`）; `CameraViewport.svelte` の canvas `aria-label`、`TouchPad.svelte:60`（`Touchscreen 320 × 240`）、`OutputPanel`/`SerialTab` の `aria-live=polite`、`aria-pressed/busy` | — |
| ハイコントラストモードのサポート | 未実装 | `web/src` に `high-contrast`/`forced-colors` 0件。`app.css` はダーク単色パレットのみ | 要対応 |

### 3.3 ブラウザサポート（Web のみ）

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| Chrome/Edge 94+、Firefox 130+、Safari 16.4+ | 未実装 | `web/src`・`web/package.json`・`+layout.svelte` に version ゲート 0件 | backend/仕様所有の受入 gate。frontend 側の実行時警告なし |

## 4. 重点機能要件

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| 4.1 ショートカットボタンは正確に 10 個 | 実装済 | `CommandsTab.svelte:12,27,302`（`ShortcutIndex = 1..10`、`shortcutIndexes`、10スロット grid） | — |
| 4.2 実行制御は開始/一時停止/再開/停止（一時停止は再開可能） | 実装済 | `CommandsTab.svelte:389-392`（Start/Pause/Resume/Stop + 状態ゲート `disabled`）; `actions.ts:89`（`POST /api/commands/control` の `start/pause/resume/stop`）; 状態バッジ `CommandsTab.svelte:271-272`（`running/paused/error/stopped`） | — |
| 4.3 LINE 通知 UI は削除、Discord Webhook のみ（旧互換メニュー項目は残存） | 実装済 | `NotificationsTab.svelte` に LINE 入力なし。`NotificationsTab.test.ts:28,53` が LINE UI 非存在を表明。`WorkspaceMenu.svelte:291-302,434-452` が LINE Token Assignment / LINE Token Check を保持し、`notifications.line_menu_behavior` が `message`（削除通知）／`noop`（無操作）を選択。`WorkspaceMenu.test.ts:173-210` が既定値・欠落時既定・noopを検証 | — |
| 4.5 PWA は将来実装 | 将来 | `web/static` なし、`manifest`/service worker 0件（仕様どおり未実装） | — |
| 4.7 テーマサポートは将来実装（Tailwind v4 は現行 Web 実装の記録） | 将来 | `app.css:3`（`@theme` は Tailwind 宣言）、`app.html:6-7`（`color-scheme: dark` 固定）、テーマ切替 UI なし。`--color-yellow: #e0af68`（`app.css:23`） | §9 も参照 |

## 5. UI レイアウト

### 5.1 全体構造

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| メニューバー＋タブコンテンツ領域＋右側パネル | 実装済 | `+page.svelte:127`（`WorkspaceMenu`）、`140`（`MainPanel`）、`179`（`RightPanel`） | — |
| メニューバーはタブ・右パネルと同列のトップレベル要素 | 実装済 | `+page.svelte:108-179`（header 内メニュー＋ `MainPanel`/`RightPanel` 配置） | — |

### 5.2 タブ構造（6メイン＋3サブ）

| # | タブ | 判定 | 直接証拠 | ギャップ・備考 |
|---|------|------|----------|----------------|
| 1 | カメラ | 実装済 | `+page.svelte:29-34`（6タブ定義・日英ラベル）; `CameraTab.svelte` | — |
| 2 | シリアル | 実装済 | 同上; `SerialTab.svelte`（ポート/ボーレート/3形式/接続切替/モニター） | — |
| 3 | 手動制御 | 実装済 | 同上; `ManualTab.svelte` | ハードウェア部は仕様どおり非表示（§6.3.2） |
| 4 | コマンド（3サブタブ付） | 実装済 | 同上; `CommandsTab.svelte:28-32`（`python/mcu/shortcuts`） | — |
| 5 | 通知 | 実装済 | 同上; `NotificationsTab.svelte`（Windows/Discord、LINE なし） | — |
| 6 | その他 | 実装済 | 同上; `OtherTab.svelte` | — |

### 5.3.1 ソフトウェアコントローラー

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| 位置 TOP/BOTTOM をその他タブで選択 | 実装済 | 値は `OtherTab.svelte:94-95,276-293` で radio 書込・表示、`RightPanel.svelte:13-15,29-42` で即時配置切替 | 未読込時の既定 `bottom`、radio 構成、変更伝播を `OtherTab.test.ts` で検証 |
| 外観 Joy-Con L シアン `#56CCF2`＋R 赤 `#E9514E` | 未実装 | `web/src` に両 hex 0件。`ControllerPanel.svelte` に該当色指定なし | テーマ token（`app.css`）で代替されているが仕様 hex と不一致 |
| アクティブ色 黄 `#FFD800` | 未実装 | `web/src` に `#FFD800` 0件。`app.css:23` の `--color-yellow: #e0af68` で代替 | 仕様 hex と不一致 |
| ホールド（押下で押下シグナル） | 実装済 | `ControllerPanel.svelte:61-66`（`press`：`setPointerCapture`＋`runtime.setGamepadButton(button, true)`） | — |
| 解放（解放で解放シグナル） | 実装済 | `ControllerPanel.svelte:69-77`（`release`：`runtime.setGamepadButton(button, false)`） | — |
| Shift+解放で `holdEndSkip`（解放シグナルを送らず視覚のみ切替） | 部分的 | `ControllerPanel.svelte:69-77`（`event.shiftKey` 時は `visuallyReleased` のみ切替、`setGamepadButton(false)` を呼ばない）; D-pad も `104-112` で同等 | `holdEndSkip` という名称がコードに存在しない。終了時・プロファイル切替時の解放フレーム送信は `input-safety.ts`/`neutralizeInput` 経由（要 backend 側確認） |
| Shift+クリックの browser 既定動作防止に `preventDefault` | 実装済 | `ControllerPanel.svelte:63,70,80,98,105,115`、`AnalogStick.svelte:57,65,71,109`、`TouchPad.svelte:30,38,44` 等 | — |
| 全標準ボタン（A/B/X/Y、L/R/ZL/ZR、MINUS/PLUS、HOME/CAPTURE、LCLICK/RCLICK、D-pad 4方向） | 実装済 | `ControllerPanel.svelte:23-54`（`buttonFields` 14種＋D-pad 4方向）; 生成型 `openapi.ts:393`（`GamepadButton` 14値） | — |
| L/R スティック 0–255 | 実装済 | `AnalogStick.svelte:33-37`（中心 128±127、0–255 に丸め）; キー操作も `79-109` で 0–255 clamp | — |
| タッチスクリーン 320×240（0-based: 0–319/0–239） | 実装済 | `TouchPad.svelte:20-26,60`（`min(319, floor(h*320))`/`min(239, floor(v*240))`、`aria-label 320 × 240`） | — |
| デッドゾーン 103–153 をニュートラル扱い | 未実装 | `web/src` に `deadzone`/`103`–`153` 関連 0件 | 要対応（frontend/ backend のどちらで持つかは製品判断） |
| rAF 同期・前回送信から 16ms 以上のみ送信・無操作時停止 | 実装済 | `AnalogStick.svelte:20,40-52`（`requestAnimationFrame(flush)`＋`timestamp - lastSentAt < 16` で再スケジュール、保留なし時は停止）; カメラ canvas 経路も `CameraViewport.svelte:220,294` で同等 | — |

### 5.3.2 出力パネル

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| 出力#1（上部）・出力#2（下部） | 実装済 | `OutputGroup.svelte:31-35`（`OutputPanel label="Output #1/#2"`） | — |
| サイズ調整スライダー 0–100 で比率決定 | 実装済 | `OtherTab.svelte:122,184-196`（`changeSplit` の 0–100 整数検証＋ range slider） | — |
| ログソース WebRTC DataChannel 優先・WebSocket フォールバック（Web/現行 Tauri 経路） | 実装済（表示側） | `CameraViewport.svelte:190-204`（フォールバック受信）、`media.ts`（`MediaTransport`/`MediaMode`）、`runtime.ts` の realtime 配線 | transport 実体は backend 所有。GPUI は対象外（仕様どおり） |
| 自動スクロール・クリア・コピー・レベルフィルタ | 実装済 | `OutputPanel.svelte:32,37-50,57,64-71,92-93`（`autoScroll`＋`localStorage` 永続、`minimum-level`、`copy()`、`onclear`） | — |
| 5レベル DEBUG/INFO/WARNING/ERROR/CRITICAL | 実装済 | `OutputPanel.svelte:15,22`（`ranks`＋`levels = ['debug','info','warning','error','critical']`） | 各レベルの意味定義は文書所有でコード対応物なし（正常） |
| その他タブ「出力をクリア」独立ボタン | 実装済 | `OtherTab.svelte` の `runtime.clearOutputs()` ボタン | — |

### 5.4 サブタブ（Commands）

| # | サブタブ | 判定 | 直接証拠 |
|---|----------|------|----------|
| 1 | Python Command | 実装済 | `CommandsTab.svelte:28-32`（`subtabs` 定義）、`114-129`（`filterForSubtab` の python/mcu 振分） |
| 2 | Mcu Command | 実装済 | 同上 |
| 3 | Shortcut（10ボタン割当 grid） | 実装済 | 同上＋`299-341`（10スロット grid＋割当フロー） |

### 5.5 ウィジェットモード（7種）

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| 7モード切替・正準値・即時反映・プロファイル対応・永続化 | 実装済 | `OtherTab.svelte:52-60,90-92,240-248`（7値＋書込）; `RightPanel.svelte:12,18-27`（`mode` による即時表示切替） | 表示ラベルが仕様と異なる（例: `ALL (default)`→`All`/`すべて`）。機能的差なし |
| 設定表面（TOML/動的パス/CLI/env/OpenAPI）は backend 所有 | 対象外 | frontend は `ui.widget_mode` の参照・送信のみ | — |
| 既定 `all`、小文字 ASCII・大文字小文字不問正規化 | 対象外 | 正規化は backend 所有。frontend 既定表示は `OtherTab.svelte:242`/`RightPanel.svelte:12` の `?? 'all'` | — |

### 5.6 コントローラー位置

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| `top`/`bottom`（既定 `bottom`）、即時反映 | 実装済 | `OtherTab.svelte:276-293`（radio 書込・既定 `bottom`）、`RightPanel.svelte:13-15,29-42`（即時配置） | `OtherTab.test.ts` が未読込時既定・選択肢・変更伝播を検証 |

### 5.7 ダイアログボタン位置

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| `top`/`bottom`/`both`（既定 `bottom`）、即時反映 | 実装済 | `OtherTab.svelte:294-311`（3値 radio・書込・既定 `bottom`）; 消費側 `ScriptUiLayer.svelte:19-22`（`dialogPosition` 導出、`bottom` フォールバック） | `OtherTab.test.ts` が構成・選択肢・変更伝播を検証 |

### 5.8 出力分割比率

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| int 0–100、既定 20、スライダー即時反映 | 実装済 | `OtherTab.svelte:117-124,184-196`（整数・範囲検証＋ slider）; `RightPanel.svelte:16`（`?? 20`） | — |
| 計算式 `output1 = 10 + 0.8*value`、`output2 = 100 - output1` | 実装済 | `OutputGroup.svelte:16-18`（コメント＋`10 + 0.8 * splitRatio`、`100 - output1Percent`） | — |
| 範囲外拒否・整数丸め | 実装済 | `OtherTab.svelte:119-123`（非整数・範囲外を拒否） | — |

### 5.9 stdout 出力先

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| `output_1`/`output_2`（既定 `output_1`）、ラジオ・即時反映 | 実装済 | `OtherTab.svelte:88-89,205-219`（正準値のみ送信の radio）; 消費側 `runtime.ts:403-407`（`ui.stdout_destination ?? 'output_1'` で振分）; 生成型 `openapi.ts:1120,1254,1397`（正準値のみ） | — |
| レガシー `"1"`/`"2"` は読取のみ・正準値へ自動変換、書出は常に正準値 | 部分的 | 書出は正準値のみ（上記）。frontend 側に `"1"`/`"2"` の読取・変換コードなし（`web/src` 0件） | 変換は backend（ランタイム）所有の可能性。要 backend 側確認 |

## 6. タブ仕様

### 6.1 カメラタブ

#### 6.1.1 映像表示

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| Canvas 描画（`CaptureArea` 互換参照可） | 実装済 | `CameraViewport.svelte:76,161-168,482-493`（`<canvas>`＋2D 描画） | `CaptureArea` 名称のコード対応物なし（旧 API 互換名の扱いは backend 側） |
| プライマリ WebRTC ビデオトラック | 実装済 | `CameraViewport.svelte:178`（`view.media.mode === 'webrtc'` 分岐）、`media.ts`（`MediaTransport`、`ontrack` 相当の pendingStream 処理） | — |
| フォールバック MJPEG over WebSocket | 実装済 | `CameraViewport.svelte:190-204`（`receiveFallback`：Blob→ObjectURL→canvas） | — |
| FPS 設定コンボボックス | 実装済 | `CameraTab.svelte:281-282`（`ui.fps`＋`ui.fps_options` から生成） | — |

#### 6.1.2 カメラ設定

| コントロール | 判定 | 直接証拠 | ギャップ・備考 |
|--------------|------|----------|----------------|
| デバイス選択（整数または文字列セレクター、`GET /api/devices/cameras` 列挙、`PATCH /api/settings` 経由） | 実装済 | `CameraTab.svelte:52-74,86-113,268-272`（列挙＋選択＋`camera.device` 書込、利用不能セレクターの `(configured, unavailable)` 表示・自動切替なし）; `actions.ts:102`（cameras 列挙）; `camera-selector.ts`（int/path 同一性：`/dev/videoN` 対応） | デバイス切替トランザクション本体（停止・保持・再適用・ロールバック）は backend 所有 |
| UI 表示 FPS（`ui.fps`/`ui.fps_options`） | 実装済 | `CameraTab.svelte:118-126,281-282`（正整数検証＋即時書込） | — |
| 取得 FPS（`camera.capture_fps`、既定 60、即時適用） | 実装済 | `CameraTab.svelte:118-126,290`（`type=number min=1`、既定 60） | 即時適用の実体は backend 所有 |
| 取得解像度（640x360/1280x720/1920x1080、即時適用） | 実装済 | `CameraTab.svelte:130-134,295`（`camera.capture_resolution` 書込） | 選択肢の表示内容は要目視確認（3値の存在はコード上あり） |
| フリップ（`none/vertical/horizontal/both`、旧 `set_flip` 互換は backend） | 実装済 | `CameraTab.svelte:135-136,302`（`camera.flip_mode` 書込） | 大文字小文字マッピングは backend 所有 |
| スクリーンショット形式（`png` 既定/`jpeg`、表示ラベル分離） | 実装済 | `CameraTab.svelte:137-138,309`（`camera.screenshot_format`、既定 `png`：`179,194`） | — |
| 起動時自動オープン・失敗時は利用不能表示＋他機能継続・暗黙切替なし | 部分的 | 失敗時 UI：`CameraTab.svelte:248-259`（非モーダルエラー＋`Retry camera`＋`Retry WebRTC`、選択肢は操作可能のまま） | 自動オープン本体・トランザクションは backend 所有。frontend のエラー表示は仕様の「対象・秘匿化済み理由・再試行」に対応 |
| 取得設定即時適用トランザクション（7手順） | 対象外 | backend 所有。frontend は設定送信のみ | — |
| デバイス切替トランザクション（8手順） | 対象外 | backend 所有。frontend は `changeDevice` 送信＋再試行 UI のみ | — |

#### 6.1.3 表示モード切替

| モード | 判定 | 直接証拠 |
|--------|------|----------|
| ライブビュー（`ui.camera.live_view_enabled`、既定 true、false で最終フレーム保持） | 実装済 | `CameraTab.svelte:144-151,315`（checkbox、既定 true）; 消費側 `CameraViewport.svelte:144,166`（停止＋最終フレーム保持） |
| ピクセル値（`ui.camera.pixel_values_visible`、既定 false） | 実装済 | `CameraTab.svelte:316`（checkbox、既定 false）。オーバーレイ描画は viewport 側 |
| ガイド（`ui.camera.guide_visible`、既定 false） | 実装済 | `CameraTab.svelte:317`（checkbox、既定 false）。オーバーレイ描画は viewport 側 |

#### 6.1.4 キャンバス上マウス操作

| アクション | 判定 | 直接証拠 | ギャップ・備考 |
|------------|------|----------|----------------|
| L/R スティック制御（ドラッグ） | 実装済 | `CameraViewport.svelte:315-329`（修飾なしドラッグ→`stick` モード、16ms スロットル `220`） | — |
| カラーピッカー（Ctrl+クリック、`preventDefault`） | 実装済 | `CameraViewport.svelte:318,330`（`ctrlKey+button0`→`pixel`＋`preventDefault`） | — |
| 範囲スクリーンショット（Ctrl+Shift+ドラッグ→`destination="captures"`） | 実装済 | `CameraViewport.svelte:316`（`capture` モード）; `CameraTab.svelte:177-189`（`saveScreenshot({destination:'captures', region})`） | — |
| 名前付き保存（Ctrl+Alt+ドラッグ。Tauri=`path`、Web=`download`、初期名 `capture_YYYYMMDD_HHMMSS`、形式初期選択＋一回上書き） | 実装済 | `CameraViewport.svelte:317`（`download` モード）; `CameraTab.svelte:166-175,193-219`（`timestampName`＝`capture_...`、desktop は `chooseNativeSavePath`→`destination:'path'`、Web は `destination:'download'`＋`triggerDownload`、実効 `screenshot_format` 使用：`179,194`）; `desktop.ts:29`（Web では native dialog 不可の明示） | 一回限り上書きの `overwrite:true` は `path` 経路にあり（`204-210`） |
| タッチ領域選択（Ctrl+右ドラッグ、クランプ・ソート・退化拒否・即時 TOML 永続） | 部分的 | `CameraViewport.svelte:315`（`ctrlKey+button2`→`touch` モード）、ヒント文 `549` | クランプ・ソート・退化拒否・永続の実体は backend 所有。frontend の送信側検証は要 backend 側確認 |

### 6.2 シリアルタブ

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| COM ポート選択（OS ネイティブ識別子、手動入力可、利用不能値の表示維持・自動選択なし、正規化・解決なし） | 実装済 | `SerialTab.svelte:29,95,149-154`（`input`＋`datalist` による手動入力、表示ラベル分離） | 重複排除・安定セレクター選択の実体は backend 列挙所有 |
| 更新ボタン（`GET /api/devices/serial-ports` 再スキャン） | 実装済 | `SerialTab.svelte:70-76,155`（`refreshPorts`→`actions.serialPorts()`）; `actions.ts:107` | — |
| 接続/切断トグル（`POST /api/serial/control`＋実効 `serial.*`） | 実装済 | `SerialTab.svelte:116-120,179-182`（Connect/Disconnect）; `actions.ts:160` | — |
| ボーレート・データ形式（3種） | 実装済 | `SerialTab.svelte:30-31,164-176`（baud number＋`default/qingpi/3ds`。`qingpi` 選択時は baud 115200 連動：`108-112`） | — |
| シリアルモニター（リアルタイム表示・自動スクロール・クリア） | 実装済 | `SerialTab.svelte:42-60,183-192`（`autoScroll`＋`localStorage` 永続、`runtime.clearSerial()`、live `view.serial`） | — |

### 6.3 手動制御タブ

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| キーボード（`input.keyboard_enabled`、既定 true） | 実装済 | `ManualTab.svelte:13,63`（checkbox、既定 true） | 無効化時の強制解放の実体は backend 所有 |
| L スティックマウス（既定 false） | 実装済 | `ManualTab.svelte:67`（checkbox、既定 false） | 同上 |
| R スティックマウス（既定 false） | 実装済 | `ManualTab.svelte:71`（checkbox、既定 false） | 同上 |
| ハードウェア制御は非表示（グレーアウト不可） | 実装済 | `ManualTab.svelte` に `hardware` 0件、`Software input sources` のみ | — |
| Switch Controller Simulator（§5.3.1 と同一構成） | 実装済 | `ManualTab.svelte:79-84`（右列の単一 Software-Controller へ誘導、`focusController`→`#software-controller`）; 実体は `RightPanel.svelte` の `ControllerPanel` | — |
| アクティブキー表示・全入力解放 | 実装済 | `ManualTab.svelte:77-81`（`keyboard_keys` 表示、`runtime.neutralizeInput()`） | — |

### 6.4 Commands タブ

#### 6.4.1.1 コマンドリスト＋タグマッチモード

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| Treeview/リスト表示・タグフィルター・列（名/タグ/説明） | 実装済 | `CommandsTab.svelte:344-384`（タグ select＋一覧、`command.name/class_name/module_path/tags` 表示） | — |
| タグマッチモード combobox（`exact/partial/prefix/suffix`、既定 `exact`、即時反映・永続） | 実装済 | `CommandsTab.svelte:169-171,352-355`（4値 select、既定 `exact`、`commands.tag_match_mode` 書込） | 表示ラベルが英語（`Exact/Partial/...`）で仕様の日本語ラベル（`完全一致 (default)` 等）と不一致。値は正準値で正しい |
| 動的カスタムタグマッチ関数（優先・解除で復帰、非永続） | 対象外 | backend 所有。frontend に対応物なし（正しい） | — |
| 設定表面（TOML/動的パス/CLI/env/OpenAPI） | 対象外 | backend 所有。frontend は combobox 送信のみ | — |

#### 6.4.1.2 タグ体系

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| `CommandInfo` 4フィールド（`name/module_path/class_name/tags`）・識別子＝`module_path`＋`class_name` | 実装済 | `CommandsTab.svelte:93-99`（`identity = module_path\0class_name`）; 生成型 `openapi.ts` の `CommandInfo` | 重複除外・WARNING 診断は backend 所有 |
| 自動/手動/動的タグの生成・統合 | 対象外 | backend 所有 | — |
| フィルター事前計算（7手順のキャッシュスナップショット） | 対象外 | backend 所有。frontend は完成済み `command_display_lists` の読出のみ（`CommandsTab.svelte:59-63`：選択タグ→`'-'` フォールバック）＋ `command_display_cache_loading` 表示（`360-361`） | — |
| backend 責務＝タグマッチング、frontend 責務＝fuse.js 絞込（`command_display_lists` を不変の元一覧にしコマンド行のみ入力、検索中セパレーター除外、空に戻したら復元・再要求なし） | 実装済 | `CommandsTab.svelte:2,67-85`（`Fuse`、keys＝name/module/class/tags、`threshold 0.4`）; `72-75`（`kind==='command'` のみ入力）; 空 query で `filtered`（＝元一覧）そのまま返却（`70`） | — |
| `@` なし先・`@` 付き後 | 実装済 | `CommandsTab.svelte:45-56`（`orderedTags` の `@` ソート） | — |
| ソート callback（`pokecon.commands.sort.callback`、1変換1実行） | 対象外 | backend 所有 | — |
| 先頭 `"-"`（フィルター無効） | 実装済 | `CommandsTab.svelte:45-47,59-63`（`'-'` 先頭＋フォールバック） | — |
| state（`tags`/`command_candidates`） | 実装済（参照側） | `CommandsTab.svelte:43,63-65,142-148`（`backendState.tags/command_candidates/command_display_lists` 参照） | 定義・保持は backend 所有 |

#### 6.4.2 ショートカットボタン

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| 10ボタン・任意コマンド割当（クリック→選択→完了） | 実装済 | `CommandsTab.svelte:27,302-341`（10スロット、割当フロー・`assignShortcut` は `module_path` 保存：`232-239`） | — |
| Shift+クリックで解除・右クリックでクリア | 実装済 | `CommandsTab.svelte:211-223`（`beginShortcutAssignment` の shift 分岐、`contextShortcut` の右クリック）＋ヒント文 `322` | — |
| ラベルに割当コマンド名表示 | 実装済 | `CommandsTab.svelte:150-153,312-313`（`shortcutLabel`） | — |
| `[shortcuts]` 永続化（プロファイル連動・ブラウザ横断） | 対象外 | backend 所有。frontend は `shortcuts.button_N` 書込のみ（`135-140,225-228`） | — |

#### 6.4.3 実行制御ボタン

| ボタン | 判定 | 直接証拠 |
|--------|------|----------|
| 開始（`action="start"`＋識別子） | 実装済 | `CommandsTab.svelte:188-195,389`（`control({action:'start', command})`）; `actions.ts:89` |
| 停止 | 実装済 | `CommandsTab.svelte:392`; `actions.ts:89` |
| 一時停止（再開可能） | 実装済 | `CommandsTab.svelte:390`; `actions.ts:89` |
| 再開 | 実装済 | `CommandsTab.svelte:391`; `actions.ts:89` |
| 再読み込み（`POST /api/commands/reload`） | 実装済 | `CommandsTab.svelte:197-209,393`; `actions.ts:96` |
| 状態表示（実行中/一時停止中/停止/エラー） | 実装済 | `CommandsTab.svelte:271-272`（`running/paused/error/stopped` バッジ） |

### 6.5 通知タブ

#### 6.5.1 Windows 通知

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| 開始/終了 checkbox（`notifications.windows.on_script_start/end`、bool、既定 false、即時反映・永続） | 実装済 | `NotificationsTab.svelte:13-14,156-168`（両 checkbox、既定 false） | 実行中スクリプト非再評価・Windows のみ送出は backend 所有 |
| テスト（`POST /api/notifications/test`＋`channel="windows"`、非設定アクション） | 実装済 | `NotificationsTab.svelte:148`（`testChannel('windows')`）; `actions.ts:121` | — |
| 非 Windows では値保持・通知なし、UI は全 platform 表示 | 実装済 | `NotificationsTab.svelte:139-140`（注意書き表示） | — |

#### 6.5.2 Discord 通知

| コントロール | 判定 | 直接証拠 | ギャップ・備考 |
|--------------|------|----------|----------------|
| Webhook URL（text input） | 実装済 | `NotificationsTab.svelte:195-218`（`password` 型＋マスク運用、置換時のみ送信、消去ボタン） | URL 形式検証は backend 所有（仕様どおり） |
| ユーザー名（オプション） | 実装済 | `NotificationsTab.svelte:227-233` | — |
| アバター URL（オプション） | 実装済 | `NotificationsTab.svelte:235-243`（`type=url`） | — |
| 開始/終了 checkbox（既定 false、未設定 URL は skip＋WARNING、即時反映） | 実装済（UI 側） | `NotificationsTab.svelte:245-261` | skip/WARNING 実体は backend 所有 |
| テスト（`channel="discord"`） | 実装済 | `NotificationsTab.svelte:190`; `actions.ts:121` | — |

#### 6.5.3 LINE 通知

| 要求 | 判定 | 直接証拠 |
|------|------|----------|
| UI 完全削除 | 実装済 | `NotificationsTab.svelte` に LINE 入力なし。`NotificationsTab.test.ts:28,53` が非存在を表明 |
| スクリプト API（`LINE_text`/`LINE_image`）維持、設定 UI なし | 対象外 | backend 所有 |

### 6.6.1 その他タブ設定グループ（12行）

| # | 行 | 判定 | 直接証拠 | ギャップ・備考 |
|---|----|------|----------|----------------|
| 1 | 出力サイズ調整 slider | 実装済 | `OtherTab.svelte:122,184-196` | — |
| 2 | stdout 出力先 radio | 実装済 | `OtherTab.svelte:205-219`（正準値のみ送信） | — |
| 3 | 出力クリア button | 実装済 | `OtherTab.svelte` の `runtime.clearOutputs()` | — |
| 4 | FPS combobox（`ui.fps_options` 生成→`ui.fps`） | 実装済 | `OtherTab.svelte:108-114,224-233` | — |
| 5 | ウィジェットモード combobox | 実装済 | `OtherTab.svelte:240-248` | ラベル差あり（§5.5） |
| 6 | コントローラー位置 radio | 実装済 | `OtherTab.svelte:276-293`（fieldset＋radio、未読込時の既定`bottom`）; `OtherTab.test.ts` が既定値・変更伝播を検証 | — |
| 7 | ダイアログボタン位置 radio | 実装済 | `OtherTab.svelte:294-311`（fieldset＋radio、`top/bottom/both`）; `OtherTab.test.ts` が構成・変更伝播を検証 | — |
| 8 | 動的設定自動リロード checkbox（`auto_reload_config`、単一 watcher 即時反映・グローバル保存） | 実装済（UI 側） | `OtherTab.svelte:319-330`（checkbox） | watcher・保存先の実体は backend 所有 |
| 9 | 最終ウィンドウ close 動作 combobox（`ask/shutdown/keep_backend`＋日本語ラベル） | 実装済 | `OtherTab.svelte:98-100,318-329`（3値＋「毎回確認/すべて終了/バックエンドを継続」） | — |
| 10 | compositing 無効 checkbox（Tauri 専用・再起動必須・Web では保持のみ） | 実装済 | `OtherTab.svelte:30-35,332-356`（保存値/現在値表示＋再起動バッジ＋Web 無効注記） | — |
| 11 | 表示言語 combobox（`ja`/`en`、即時反映・グローバル保存） | 実装済 | `OtherTab.svelte:101-103,250-263`（日本語/English、即時 `t()` 切替：`66-68`） | — |
| 12 | サーバー設定群（§6.7。注: 仕様の表は11行＋§6.7 で計12項目相当） | 実装済 | §6.7 参照 | — |

### 6.7 サーバー設定

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| 6.7.1 `server.web_dir`（picker/テキスト、現在値 read-only＋保存値、再起動注記、次回起動反映・現動作不変） | 実装済 | `OtherTab.svelte:37-40,128-136,375-393`（`savedWebDirectory`、現在/コピー表示、再起動注記）; `openapi.ts` の `server.web_dir` | 解決・検証・scope の実体は backend 所有 |
| 6.7.2 `server.port`（1–65535 数値、再起動注記） | 実装済 | `OtherTab.svelte:41-43,139-146,396-410`（`changePort` の非整数・範囲外拒否、現在値表示、min/max） | バインド失敗時 no-fallback、CORS 導出は backend 所有 |
| 6.7.3 `server.bind_address`（IP 入力、再起動＋LAN 無認証警告） | 実装済 | `OtherTab.svelte:44-47,148-155,411-429`（`isLoopback` 判定＋`lanExposed` 警告表示、保存値/現在値） | IP リテラル検証・scope の実体は backend 所有 |
| 6.7.4 STUN（空＝無効、不正値不保存、グローバル、以後接続から使用・既存再ネゴなし、全表面対応） | 部分的 | `OtherTab.svelte:431-447`（テキスト入力、空無効・以後接続の注記表示） | URI 形式検証・保存拒否の実体は backend 所有。frontend 側の事前検証なし（仕様の所有分担どおり） |

## 9. テーマサポート（将来）

| 要求 | 判定 | 直接証拠 |
|------|------|----------|
| 9.1 組込テーマ（ライト/ダーク/システム自動検出） | 将来 | 切替 UI なし。`app.html:6` は `dark` 固定、`app.css` はダーク単色 |
| 9.2 カスタムテーマ（ユーザー配色・CSS 変数） | 将来 | 同上。`app.css:3-27` の `@theme` 変数は基盤のみで切替機構なし |

## 11.5.7 動的設定ファイル UI

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| 11.5.7.1 メニュー配置（メニュー/コマンド/設定予約/File/終了/ヘルプの tree） | 部分的 | `WorkspaceMenu.svelte:306-458`（`<details>` dropdown の Menu/Help、動的設定 Load/Reload、設定 dir、launcher、画面リセット、GitHub/Guide/質問/更新履歴/LICENSE/version `openapi.info.version`）。実装済み側: `.bat` ランチャー生成は実装済み（Windows のみ有効）— `generateLauncher`（`WorkspaceMenu.svelte:231-267`）、UI `:392-421`（`.bat を保存`/`.bat をダウンロード`、非 Windows では無効化 `:403,409,417-421`）、`actions.ts:133-134,144-145`（`POST /api/profiles/generate-launcher`）、tests `WorkspaceMenu.test.ts:46-54,124-141,143-169`。Open Config Directory も実装済み（`WorkspaceMenu.svelte:215-229` / `:370-389`）で、プロファイル切替 UI（`:317-346`）も存在する | 仕様 tree と差あり（残存欠落）: Discord Setting Assignment/Check、File メニュー分離、終了、設定（予約）、バージョン確認アクション。アップデート確認は実装済み（`checkUpdate` `:269-284`、Help 内ボタン `:443-448`、test `:134-140`）。version は受動表示（`:437`）のみで確認アクションなし。Open Config Directory・プロファイル切替はプロファイルディレクトリ生成とは別物であり、混同しないこと（生成実体は backend 所有）。Tkinter 互換の取捨ては製品判断が必要 |
| 11.5.7.2 ファイル選択・自動判別（`.py`→Python、`.lua`→Lua、不明拡張子は副作用なし拒否＋対応拡張子表示＝`422` 契約相当） | 実装済 | `WorkspaceMenu.svelte:139-159,352-360`（`.py`/`.lua` 判別、`accept` 属性、不明時は選択拒否＋`.py/.lua` 表示）; `WorkspaceMenu.test.ts:99-122`（拒否の test） | `422` 本体は連携 API 所有 |
| 11.5.7.3 リロード種別（手動＝明示リトライ・watcher/メニュー維持、自動＝既定無効・debounce・single-flight、再起動もリトライ、`auto_reload` 有効化） | 部分的 | 手動 `reloadDynamicConfig`（`WorkspaceMenu.svelte:184-191`、単一 `POST /api/dynamic-config/control`：`actions.ts:114`）。自動の UI checkbox は `OtherTab.svelte:319-330`（`auto_reload_config`） | debounce・single-flight・既定無効・watcher 維持の実体は backend 所有。frontend の設定面は実装済みだが、backend watcher は未実装（`docs/TRACEABILITY_BACKEND.md` §11.5.1） |

## 13. クライアント側ストレージ

| 要求 | 判定 | 直接証拠 | ギャップ・備考 |
|------|------|----------|----------------|
| ショートカット割当を `localStorage` に保存しない（`settings.toml [shortcuts]` に永続） | 実装済 | `CommandsTab.svelte:225-239`（`shortcuts.button_N` を `runtime.writeSettings` へ）。`web/src` の `localStorage` 使用は `OutputPanel.svelte:37-50`（auto-scroll/minimum-level）と `SerialTab.svelte:51,60`（serial auto-scroll）のみ | — |
| 仕様化されたクライアント永続項目なし。一時状態のメモリ保持は可、設定の永続先にしない | 実装済 | 上記のとおり `localStorage` は ephemeral な表示 prefs のみ（output auto-scroll/level、serial auto-scroll）。設定・shortcut の `localStorage` 保存なし | — |

## A. Tkinter UI リファレンス（非規範）

仕様が「元の UI の記録」と位置づける参考章。実装の照合対象ではなく、以下の対応を参考として記録する。

| 記録 | 対応する現行実装 |
|------|------------------|
| CameraTab（VideoCapture・canvas・マウス操作） | `CameraTab.svelte`＋`CameraViewport.svelte`（canvas＋5 mouse chord） |
| SerialTab（COM・9600/115200・3形式・モニター） | `SerialTab.svelte:164-176`（baud＋3形式）、モニター `183-192` |
| ManualControlTab（software checkbox・hardware・simulator） | `ManualTab.svelte`（software のみ、hardware 非表示は仕様 §6.3.2 どおり） |
| CommandTab（3サブタブ・タグ・10ボタン・5実行ボタン） | `CommandsTab.svelte`（全対応） |
| NotificationTab（Discord＋Windows、LINE 削除） | `NotificationsTab.svelte`（全対応） |
| OthersTab（slider・stdout radio・クリア・7 mode・位置 radio・dialog 位置） | `OtherTab.svelte`（位置・dialog位置は radio、§5.6/§5.7） |
| 色 `#56CCF2`/`#E9514E`/`#FFD800`、デッドゾーン、320×240 | 色・デッドゾーンは未実装（§5.3.1）、320×240 は実装済 |

## アクション可能なギャップ一覧

1. スティックデッドゾーン 103–153 の扱いが frontend に存在しない（§5.3.1）。所有先（frontend/backend）の決定が必要。
2. Joy-Con 色・アクティブ色が仕様 hex と不一致（`#56CCF2`/`#E9514E`/`#FFD800` なし、`--color-yellow: #e0af68` で代替）（§5.3.1/A.2）。
3. `holdEndSkip` が名称として存在せず、Shift+解放の意味論のみ実装（§5.3.1）。終了時・プロファイル切替時の解放保証は backend 側の確認が必要。
4. レガシー stdout `"1"`/`"2"` の読取・自動変換が frontend にない（書出は正準値のみ正しい）（§5.9）。backend 所有の可能性あり、要確認。
5. タグマッチモードの表示ラベルが英語で仕様の日本語ラベルと不一致（値は正しい）（§6.4.1.1）。
6. ハイコントラスト・ブラウザ version ゲート・遅延表明が frontend にない（§3.1–§3.3）。
7. メニュー tree が仕様と不一致（Discord 互換項目・終了・設定予約・バージョン確認等の欠落）（§11.5.7.1）。Tkinter 互換の取捨ては製品判断が必要。
