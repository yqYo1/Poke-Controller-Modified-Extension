# Poke-Con EX 開発計画

## 前提条件

| 項目 | 決定 |
|------|------|
| フレームワーク | SvelteKit + Svelte 5（runes mode） |
| スタイリング | Tailwind CSS |
| UIコンポーネント | shadcn-svelte（必要になったら導入） |
| 状態管理 | Svelte 5 runes |
| 型定義 | Rust側utoipa → OpenAPIスキーマ → TypeScript型自動生成 |
| 型生成出力 | `web/src/lib/api/types.ts` に自動生成 |
| Tauri統合 | `src-tauri/` を修正・維持 |
| 既存JSコード | **完全削除済み**（参照しない） |
| Tkinter機能 | 準拠（8タブ構成、10ショートカット、Pause/Restart等） |
| Line通知UI | **削除**（script APIのみ維持） |
| nix統合 | すべてのツールをnix flake経由で実行 |
| CI整備 | 完了済み |

## 通信方式

| 通信内容 | プライマリ | フォールバック |
|---------|-----------|---------------|
| カメラ映像 | WebRTC video track | MJPEG over HTTP |
| コントローラー入力 | WebRTC DataChannel | WebSocket |
| ログ | WebRTC DataChannel | WebSocket |
| その他API | HTTP REST API | — |

---

## フェーズ詳細

### フェーズ1-9: UIリファクタリング ✅ 完了（簡素化）

| フェーズ | 内容 | 状態 |
|---------|------|------|
| 1 | SvelteKit環境構築 + 現行JS完全削除 + nix統合 | ✅ |
| 2 | APIクライアント移行（TypeScript化 + OpenAPI生成） | ✅ |
| 3 | TypeScript CI整備 | ✅ |
| 4 | コンポーネント再実装（SPA方式、Tkinter準拠、8タブ） | ✅ |
| 5 | カメラ映像配信（MJPEG + WebRTC） | ✅ |
| 6 | ビルド・統合 | ✅ |
| 7 | 追加機能（キーコンフィグ/PokemonHome等） | ✅ |
| 8 | PWA対応 | ✅ |
| 9 | テーマ機能 | ✅ |

**詳細**: 各フェーズの詳細タスクは過去のコミット履歴を参照。

---

### フェーズ10: 重大バグ修正 🔴 優先度：最高

**目的**: メモリ安全性と安定性のクリティカル問題を解決

#### 10.1 メモリ安全性（P0）

| タスクID | 内容 | ファイル | 問題 |
|---------|------|---------|------|
| 10.1.1 | `unsafe transmute` 除去 | `rust/pokecon-core/src/cv/backends.rs:40-46` | ライフタイム `'static` に延長 → use-after-free リスク |
| 10.1.2 | Lua デッドロック修正 | `rust/pokecon-core/src/lua/runtime.rs` | ロック保持中にコールバック呼出 → デッドロック |
| 10.1.3 | PythonCommand ランタイム統合 | `rust/pokecon-pybindings/src/python_cmd.rs` | 各メソッドが個別に `Runtime::new()` → リソース枯渇 |

#### 10.2 ボタン認識バグ（P0）

| タスクID | 内容 | ファイル | 問題 |
|---------|------|---------|------|
| 10.2.1 | `+` セパレータと `PLUS` ボタン名の衝突解消 | `rust/pokecon-core/src/serial/format.rs` | `"PLUS"` が `["PL","US"]` に分割される |

#### 10.3 モジュール分割（P0）

| タスクID | 内容 | ファイル | 問題 |
|---------|------|---------|------|
| 10.3.1 | `main.rs` モジュール分割 | `src-tauri/src/main.rs` (2,676行) | 保守性低下、テスト不可能 |

**依存関係**: 10.1.1 → 10.1.2 → 10.1.3（順次実施）
**10.2.1 と 10.3.1 は並列実行可能**

---

### フェーズ11: PyO3バインディング実装 🔴 優先度：高

**目的**: Python互換層のコア機能を実装

#### 11.1 シリアル通信バインディング

| タスクID | 内容 | 未実装API |
|---------|------|----------|
| 11.1.1 | `pokecon.sender` モジュール実装 | `Sender.open()`, `.close()`, `.is_opened()`, `.write_row()`, `.write_list()` 等 8API |
| 11.1.2 | `pokecon.keys` 拡張 | `Stick`, `Tilt`, `Direction`, `Touchscreen`, `SendFormat`, `KeyPress`, `Button.convert()` 等 8API |

#### 11.2 コマンド・画像処理バインディング

| タスクID | 内容 | 未実装API |
|---------|------|----------|
| 11.2.1 | `pokecon.command` 実装 | `press()`, `hold()`, `holdEnd()`, `wait()`, `finish()` 等 20API |
| 11.2.2 | `pokecon.image_proc` 実装 | `isContainTemplate`, `isContainTemplate_max`, `saveImage`, `getImage` 等 3API |

#### 11.3 通知・ネットワークバインディング

| タスクID | 内容 | 未実装API |
|---------|------|----------|
| 11.3.1 | `pokecon.notify` 実装 | Discord/LINE通知 全API |
| 11.3.2 | `pokecon.net` 実装 | Socket/MQTT 全API |

**依存関係**: 11.1.1 → 11.1.2 → 11.2.1 → 11.2.2 → 11.3.1 → 11.3.2（順次）
**前提**: フェーズ10の `python_cmd.rs` ランタイム統合完了後に実施

---

### フェーズ12: Lua API実装 🟡 優先度：中

**目的**: Luaスクリプト機能の実装

| タスクID | 内容 | ファイル | 問題 |
|---------|------|---------|------|
| 12.1 | Lua API関数実装 | `rust/pokecon-core/src/lua/api.rs` | 全関数が `println!` スタブ |
| 12.2 | `lua_value_to_json()` 修正 | `rust/pokecon-core/src/lua/api.rs` | `format!("{:?}")` で非効率・不正確 |

**前提**: フェーズ10のLuaデッドロック修正完了後に実施

---

### フェーズ13: Python互換層完成 🟡 優先度：中

**目的**: Pythonスクリプトの完全互換性

#### 13.1 空実装・スタブ解消

| タスクID | 内容 | ファイル |
|---------|------|---------|
| 13.1.1 | `_adapter.py` 実装 | `python/pokecon/_adapter.py` — `press_button()`, `template_match()` |
| 13.1.2 | `events.py` 実装 | `python/pokecon/events.py` — 8関数全て `...` スタブ |
| 13.1.3 | `commands.py` スタブクラス実装 | `_SocketStub`(5メソッド), `_MQTTStub`(8メソッド) |
| 13.1.4 | `keys.py` 型修正 | `Button` を `str` → `IntFlag`, `Hat` を `str` → `IntEnum` |

#### 13.2 ダイアログ・UI連携

| タスクID | 内容 | ファイル |
|---------|------|---------|
| 13.2.1 | `dialogue()` 実装 | `python/pokecon/commands.py:227-243` |
| 13.2.2 | `dialogue6widget()` 実装 | `python/pokecon/commands.py` |

**前提**: フェーズ11のPyO3バインディング完了後に実施

---

### フェーズ14: ネットワーク・イベントシステム修正 🟡 優先度：中

**目的**: 安定性と信頼性の向上

| タスクID | 内容 | ファイル | 問題 |
|---------|------|---------|------|
| 14.1 | MQTT接続確認実装 | `rust/pokecon-core/src/net/mqtt.rs` | 接続成功確認なしで `Ok` 返却 |
| 14.2 | SocketClient クリーンクローズ | `rust/pokecon-core/src/net/socket.rs` | `shutdown()` 未呼び出し |
| 14.3 | MQTT クリーンクローズ | `rust/pokecon-core/src/net/mqtt.rs` | `handle.abort()` で強制終了 |
| 14.4 | EventBus 伝搬停止修正 | `rust/pokecon-core/src/events/bus.rs` | `stop_propagation()` が動作しない |
| 14.5 | `interframe_diff` 再帰修正 | `rust/pokecon-core/src/cv/image_processing.rs` | フォーマット不一致時の再帰呼び出し |

**並列実行可能**

---

### フェーズ15: リファクタリング・品質向上 🟢 優先度：低

**目的**: 保守性とパフォーマンスの向上

#### 15.1 重複コード除去

| タスクID | 内容 | ファイル |
|---------|------|---------|
| 15.1.1 | `parse_buttons` 統合 | 2箇所で重複実装 |
| 15.1.2 | `format_default_row()` / `SendFormat::convert_to_default()` 統合 | 重複ロジック |
| 15.1.3 | `frame_to_base64_jpeg` / `save_frame_as_jpeg` 統合 | 共通ヘルパー抽出 |

#### 15.2 設定システム統合

| タスクID | 内容 | ファイル |
|---------|------|---------|
| 15.2.1 | 設定読み込みパス統合 | `main.rs` / `settings.rs` |
| 15.2.2 | Pythonバージョン統一 | `pyrightconfig.json`, `ruff.toml`, `pyproject.toml` |

#### 15.3 命名・構造改善

| タスクID | 内容 | ファイル |
|---------|------|---------|
| 15.3.1 | `WindowsNotifier` → `DesktopNotifier` 改名 | `rust/pokecon-core/src/notify/windows.rs` |
| 15.3.2 | `_3dsController` 命名修正 | `rust/pokecon-core/src/serial/keypress.rs:110` |
| 15.3.3 | Camera 間接参照最適化 | `rust/pokecon-core/src/cv/camera.rs` |

#### 15.4 テスト追加

| タスクID | 内容 | 対象 |
|---------|------|------|
| 15.4.1 | `src-tauri` テスト追加 | `main.rs`, `webrtc.rs`, `vaapi_encoder.rs` |
| 15.4.2 | `pokecon-pybindings` テスト追加 | `events.rs`, `keys.rs`, `python_cmd.rs` |

**並列実行可能**

---

## 実装順序（依存関係考慮）

```
フェーズ1-9 ✅ 完了
  ↓
フェーズ10 🔴 重大バグ修正
  ├── 10.1.1 unsafe transmute 除去
  ├── 10.1.2 Lua デッドロック修正
  ├── 10.1.3 PythonCommand ランタイム統合
  ├── 10.2.1 +/PLUS 衝突解消
  └── 10.3.1 main.rs モジュール分割
  ↓
フェーズ11 🔴 PyO3バインディング実装
  ├── 11.1.1 pokecon.sender 実装
  ├── 11.1.2 pokecon.keys 拡張
  ├── 11.2.1 pokecon.command 実装
  ├── 11.2.2 pokecon.image_proc 実装
  ├── 11.3.1 pokecon.notify 実装
  └── 11.3.2 pokecon.net 実装
  ↓
フェーズ12 🟡 Lua API実装
  ├── 12.1 Lua API関数実装
  └── 12.2 lua_value_to_json() 修正
  ↓
フェーズ13 🟡 Python互換層完成
  ├── 13.1.1 _adapter.py 実装
  ├── 13.1.2 events.py 実装
  ├── 13.1.3 commands.py スタブ実装
  ├── 13.1.4 keys.py 型修正
  ├── 13.2.1 dialogue() 実装
  └── 13.2.2 dialogue6widget() 実装
  ↓
フェーズ14 🟡 ネットワーク・イベントシステム修正
  ├── 14.1 MQTT接続確認
  ├── 14.2 SocketClient クリーンクローズ
  ├── 14.3 MQTT クリーンクローズ
  ├── 14.4 EventBus 伝搬停止修正
  └── 14.5 interframe_diff 再帰修正
  ↓
フェーズ15 🟢 リファクタリング・品質向上
  ├── 15.1 重複コード除去
  ├── 15.2 設定システム統合
  ├── 15.3 命名・構造改善
  └── 15.4 テスト追加
```

**並列実行可能な組み合わせ**:
- 10.2.1 と 10.3.1（フェーズ10内）
- フェーズ14の全タスク（14.1-14.5）
- フェーズ15の全タスク（15.1-15.4）

---

## 未実施タスク一覧

### 🔴 優先度：最高（メモリ安全性・安定性）

| # | タスク | フェーズ |
|---|------|---------|
| 1 | unsafe transmute 除去 | 10.1.1 |
| 2 | Lua デッドロック修正 | 10.1.2 |
| 3 | PythonCommand ランタイム統合 | 10.1.3 |
| 4 | +/PLUS ボタン名衝突解消 | 10.2.1 |
| 5 | main.rs モジュール分割 | 10.3.1 |

### 🔴 優先度：高（コア機能）

| # | タスク | フェーズ |
|---|------|---------|
| 6 | PyO3 sender モジュール実装 | 11.1.1 |
| 7 | PyO3 keys 拡張 | 11.1.2 |
| 8 | PyO3 command 実装 | 11.2.1 |
| 9 | PyO3 image_proc 実装 | 11.2.2 |
| 10 | PyO3 notify 実装 | 11.3.1 |
| 11 | PyO3 net 実装 | 11.3.2 |

### 🟡 優先度：中（機能完成）

| # | タスク | フェーズ |
|---|------|---------|
| 12 | Lua API関数実装 | 12.1 |
| 13 | lua_value_to_json() 修正 | 12.2 |
| 14 | _adapter.py 実装 | 13.1.1 |
| 15 | events.py 実装 | 13.1.2 |
| 16 | commands.py スタブ実装 | 13.1.3 |
| 17 | keys.py 型修正 | 13.1.4 |
| 18 | dialogue() 実装 | 13.2.1 |
| 19 | MQTT接続確認 | 14.1 |
| 20 | SocketClient クリーンクローズ | 14.2 |
| 21 | EventBus 伝搬停止修正 | 14.4 |

### 🟢 優先度：低（品質向上）

| # | タスク | フェーズ |
|---|------|---------|
| 22 | 重複コード除去 | 15.1 |
| 23 | 設定システム統合 | 15.2 |
| 24 | 命名・構造改善 | 15.3 |
| 25 | テスト追加 | 15.4 |

---

## 注意事項

- **既存のReactコード（web-old/含む）は参照しない**
- **すべてのツールはnix flake経由で実行**
- **優先度：低のフェーズも実装必須**（後回しにするだけでスキップしない）
- **unsafeコードの追加は禁止**（既存のunsafe除去を優先）
- **PyO3バインディング実装時は、既存Pythonスクリプトとの互換性を最優先**

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
- コミットメッセージにはフェーズ番号と主要な変更内容を記載する（例: `fix(rust): phase 10.1.1 - remove unsafe transmute in backends.rs`）
- 中間的な作業途中状態はコミットせず、フェーズ内のタスクが完了してからコミットする
- push前には必ず `nix run .#check` を実行し、CIが通ることを確認する
