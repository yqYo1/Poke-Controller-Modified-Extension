# Poke-Controller Modified Extension — UI Refactoring Specification

> **Version**: 2.0.0-draft  
> **Branch**: `refactor/rust-core`  
> **Date**: 2026-05-21  
> **Scope**: Web/Desktop UI (SvelteKit) — Backend API and Rust core are out of scope  
> **Source**: Past user requirements extracted from session transcripts (not current codebase)

---

## 1. Overview

### 1.1 Purpose

This document specifies the requirements for the new web/desktop UI of Poke-Controller Modified Extension, replacing the legacy Python/Tkinter UI. The specification is derived **exclusively from past user requirements** communicated during refactoring sessions, not from the current codebase.

### 1.2 Design Philosophy

- **Visual parity with Tkinter**: The new UI must closely match the original Tkinter layout and appearance, not merely replicate features functionally. The layout, colors, button spacing, and widget types must align with the original.
- **Script compatibility**: All scripts that worked with the pre-refactoring version must continue to work normally. No breaking changes to the script API.
- **Modern stack**: SvelteKit + Svelte 5 (runes mode) + **Tailwind CSS v4** (confirmed, not subject to change).
- **Low-latency communication**: WebRTC primary with HTTP/MJPEG and WebSocket fallbacks. WebSocket auto-reconnect every 3 seconds on disconnect.
- **Type safety**: OpenAPI-generated TypeScript types from Rust backend via `utoipa` v5 + `openapi-typescript`.
- **No authentication**: The application is designed for local/LAN use only. No API authentication required.
- **React code fully removed**: Requirements state the React frontend codebase is "garbage" and must never be referenced. The specification derives from Tkinter layout requirements only.

### 1.3 Target Platforms

| Platform | UI Mode | Notes |
|----------|---------|-------|
| Desktop (Windows/Linux) | Tauri (WebView wrapper) | Shares axum HTTP server with web mode |
| Web browser | Standalone SvelteKit SPA | Served by axum HTTP server |
| Mobile (future) | Responsive SPA | Same codebase, adaptive layout |

---

## 2. UI Layout (Tkinter Parity)

### 2.1 Overall Structure

```
+-------------------------------------------------------------+
|  +----------------------+  +-----------------------------+  |
|  |                      |  |                             |  |
|  |   Tab Content Area   |  |   Right Side Panel          |  |
|  |   (Notebook/TabView) |  |                             |  |
|  |                      |  |  +-----------------------+  |  |
|  |  [Camera]             |  |  |  Software Controller  |  |  |
|  |  [Serial]             |  |  |  (Joy-Con layout)     |  |  |
|  |  [Manual Control]     |  |  +-----------------------+  |  |
|  |  [Commands]           |  |                             |  |
|  |  [Notification]       |  |  +-----------------------+  |  |
|  |  [Others]             |  |  |  Output #1            |  |  |
|  |                       |  |  +-----------------------+  |  |
|  |                       |  |                             |  |
|  |                       |  |  +-----------------------+  |  |
|  |                       |  |  |  Output #2            |  |  |
|  |                       |  |  +-----------------------+  |  |
|  +----------------------+  +-----------------------------+  |
+-------------------------------------------------------------+
```

### 2.2 Tab Structure (6 Main Tabs + 3 Sub-tabs)

> **Note on tab count**: This specification describes **6 main tabs** at the top level. The Commands tab contains **3 sub-tabs** (Python Command, Mcu Command, Shortcut), which brings the total to 9 distinct tabbed interfaces. PLAN.md references an "8-tab structure" which counts the Commands sub-tabs differently. This specification consistently uses "6 main tabs" to refer to the top-level notebook tabs.

| # | Tab Name | Priority | Description |
|---|----------|----------|-------------|
| 1 | **Camera** | High | Video feed display (Canvas/CaptureArea), device selection, FPS, flip, display mode toggle, mouse-based stick control and screenshot |
| 2 | **Serial** | High | COM port selection, baud rate, data format (3 types), connect/disconnect, serial monitor |
| 3 | **Manual Control** | High | Software Control (keyboard, mouse stick emulation), Hardware Control (ProController/Xinput, recording), Switch Controller Simulator with full Joy-Con layout |
| 4 | **Commands** | High | 3 sub-tabs (Python Command, Mcu Command, Shortcut), command list with tag filter, 10 shortcut buttons, execution control (Start/Pause/Restart/Stop/Reload) |
| 5 | **Notification** | Medium | Windows notification settings, Discord webhook (URL, username, avatar), LINE UI fully removed |
| 6 | **Others** | Medium | Output size adjuster, stdout destination, widget mode selector, software controller position, dialogue button position, clear outputs |

### 2.3 Right Side Panel

#### 2.3.1 Software Controller (Joy-Con Layout)

- **Position**: Configurable TOP/BOTTOM within right panel (radio button selection in Others tab).
- **Appearance**: Joy-Con L (cyan `#56CCF2`) + R (red `#E9514E`) layout.
- **Active color**: Yellow `#FFD800` when button is actively pressed/held.
- **Input methods**:
  - **Hold**: `<Button-1>` press triggers button hold (sends press signal).
  - **Release**: `<ButtonRelease-1>` triggers button release (sends release signal).
  - **Shift+Release**: Triggers `holdEndSkip` — only toggles visual state without sending release signal to the backend.
- **Buttons**: All standard Switch controller buttons:
  - A, B, X, Y
  - L, R, ZL, ZR
  - MINUS (−), PLUS (+)
  - HOME, CAPTURE
  - D-pad (Up, Down, Left, Right)
  - L-stick (analog, 0–255 coordinates)
  - R-stick (analog, 0–255 coordinates)
  - Touch screen simulation (320×240 coordinate input)
- **Analog sticks**: 0–255 range on both X and Y axes. Center position has a ±10% dead zone (values 103–153 are treated as neutral).

#### 2.3.2 Output Panels

- **Output #1**: Primary log/output display.
- **Output #2**: Secondary log/output display.
- **Sizing**: Controlled by "Output Size Adjuster" slider (0–100) in Others tab. Determines proportional split between Output#1 and Output#2.
- **Log source**: Logs received via WebSocket from backend.
- **Features**: Auto-scroll, clear button, copy to clipboard, log level filtering.
- **Standalone button**: "Clear Outputs" button in Others tab.

### 2.4 Sub-tab Structure (Commands Tab)

The Commands tab has 3 sub-tabs (internal tabs):

| # | Sub-tab | Description |
|---|---------|-------------|
| 1 | **Python Command** | List/tree of available Python command scripts |
| 2 | **Mcu Command** | List/tree of available MCU command scripts |
| 3 | **Shortcut** | 10 shortcut button assignment grid |

---

## 3. Widget Modes (7 Types)

The UI must support 7 display combinations for the right side panel, selectable via a combo box in the Others tab:

| Mode | Software Controller | Output #1 | Output #2 | Description |
|------|---------------------|-----------|-----------|-------------|
| 1 | Show | Show | Show | Full panel (default) |
| 2 | Show | Show | Hide | Single output |
| 3 | Show | Hide | Show | Single output (swapped) |
| 4 | Hide | Show | Show | Outputs only |
| 5 | Show | Hide | Hide | Controller only |
| 6 | Hide | Show | Hide | Output #1 only |
| 7 | Hide | Hide | Show | Output #2 only |

---

## 4. Tab Specifications

### 4.1 Camera Tab

#### 4.1.1 Video Feed Display

- **Display method**: Canvas element (CaptureArea) for video rendering.
- **Primary stream**: WebRTC video track (low latency).
- **Fallback**: MJPEG over HTTP (`<img>` tag or equivalent) if WebRTC unavailable.
- **Frame rate**: Configurable via FPS setting (Spinbox or Combobox).

#### 4.1.2 Camera Settings

| Control | Type | Description |
|---------|------|-------------|
| **Camera device selection** | Combobox | Dropdown of available camera devices |
| **FPS** | Combobox | Configurable frames per second (1–30fps) |
| **Flip** | Checkbox | Horizontal/vertical flip toggle |

#### 4.1.3 Display Mode Toggles (Checkboxes)

| Mode | Description |
|------|-------------|
| **Realtime** | Live video feed display |
| **Value** | Display numeric pixel values or overlay data |
| **Guide** | Display guide overlays or reference lines |

These are checkboxes for display overlay toggling.

#### 4.1.4 Mouse Operations on Canvas

The Camera canvas (CaptureArea) supports the following mouse interactions:

| Action | Trigger | Behavior |
|--------|---------|----------|
| **LStick/RStick control** | Mouse drag on canvas | Emulates left/right analog stick movement based on drag direction/distance |
| **Color picker** | Ctrl+Click | Picks color value at click position |
| **Range screenshot** | Ctrl+Shift+Drag | Captures screenshot of selected rectangular region on canvas |
| **Named save** | Ctrl+Alt+Drag | Saves selected region to a named file prompt |

#### 4.1.5 Screenshot Capture

- **Save location**: `./Captures/` directory.
- **Format**: PNG/JPEG (selectable).

#### 4.1.6 Camera Backend

- **Backend**: OpenCV (`cv2.CAP_DSHOW` on Windows, `cv2.CAP_V4L2` on Linux).
- **Threading**: Frame capture runs in separate thread.

### 4.2 Serial Tab

#### 4.2.1 Connection Control

| Control | Type | Description |
|---------|------|-------------|
| **COM port selection** | Combobox | Dropdown of available COM/Serial ports |
| **Refresh button** | Button | Rescan available ports |
| **Connect/Disconnect** | Toggle button | Connect or disconnect from selected port |

#### 4.2.2 Configuration

| Setting | Options | Default |
|---------|---------|---------|
| **Baud Rate** | 9600 / 115200 | 9600 |
| **Data Format** | Default / Qingpi / 3DS Controller | Default |

- **Default format**: Baud rate 9600.
- **Qingpi format**: Baud rate 9600.
- **3DS Controller format**: Baud rate 115200.

#### 4.2.3 Serial Monitor

- **Component**: Text widget with Scrollbar.
- **Functionality**: Displays incoming/outgoing serial data in real-time.
- **Features**: Auto-scroll to latest entry, clear button.

### 4.3 Manual Control Tab

#### 4.3.1 Software Control Section

| Control | Type | Description |
|---------|------|-------------|
| **Keyboard** | Checkbox | Enables keyboard-based controller input (global hotkeys) |
| **LStick Mouse** | Checkbox | Enables mouse emulation of left analog stick on canvas |
| **RStick Mouse** | Checkbox | Enables mouse emulation of right analog stick on canvas |

#### 4.3.2 Hardware Control Section

| Control | Type | Description |
|---------|------|-------------|
| **ProController** | Radio (with Xinput) | Switch Pro Controller input mode |
| **Xinput** | Radio (with ProController) | Xbox-compatible controller input mode |
| **Record** | Checkbox | Enables input recording |

#### 4.3.3 Switch Controller Simulator

Full Joy-Con style button layout rendered in the tab content area:

- **D-Pad** (Up, Down, Left, Right — directional cross)
- **L / ZL** (shoulder buttons, left side)
- **MINUS (−) / CAPTURE** (small buttons, left center)
- **A / B / X / Y** (face buttons, right side)
- **R / ZR** (shoulder buttons, right side)
- **PLUS (+) / HOME** (small buttons, right center)
- **LSTICK** (clickable analog stick, left side, 0–255 coordinates, ±10% dead zone)
- **RSTICK** (clickable analog stick, right side, 0–255 coordinates, ±10% dead zone)
- **Touch screen** (320×240 coordinate grid for touch emulation)

### 4.4 Commands Tab

#### 4.4.1 Sub-tab Structure

The Commands tab contains 3 sub-tabs (internal tab switching):

| Sub-tab | Content |
|---------|---------|
| **Python Command** | Lists available Python command scripts |
| **Mcu Command** | Lists available MCU command scripts |
| **Shortcut** | 10 shortcut button assignment grid |

#### 4.4.2 Command List

- **Display**: Listbox or Treeview showing available commands.
- **Tag filter**: Dropdown or combobox to filter commands by label/tag.
- **Columns**: Command name, tags, description (if Treeview).

#### 4.4.3 Shortcut Buttons (10 Buttons)

- **Count**: 10 shortcut buttons (requirement changed from original 4 buttons to 10).
- **Assignment**: User-assignable to any loaded command (click to assign, Shift+Click to assign).
- **Clear**: Right-click to clear assignment.
- **Display**: Button label shows assigned command name.
- **Keyboard shortcuts**: F1–F10 or other assignable hotkeys.
- **Storage**: Settings saved in `localStorage`.

#### 4.4.4 Execution Control

| Button | Keyboard Shortcut | Action |
|--------|-------------------|--------|
| **Start** | F5 | Begin command execution |
| **Pause** | Shift+F6 | Pause execution (resumable) |
| **Restart** | — | Restart from beginning |
| **Stop** | Escape | Abort execution immediately |
| **Reload** | — | Reload command list from filesystem |

- **Status display**: Running / Paused / Stopped / Error.
- **Progress**: Progress bar for supported commands.

### 4.5 Notification Tab

#### 4.5.1 Windows Notification

| Control | Type | Description |
|---------|------|-------------|
| **Notify on script start** | Checkbox | Send Windows notification when script execution starts |
| **Notify on script end** | Checkbox | Send Windows notification when script execution ends |
| **Test** | Button | Send a test notification to verify configuration |

#### 4.5.2 Discord Notification

| Control | Type | Description |
|---------|------|-------------|
| **Webhook URL** | Text input | Discord webhook URL with validation |
| **Username** | Text input | Custom username for Discord messages (optional) |
| **Avatar URL** | Text input | Custom avatar image URL for Discord messages (optional) |
| **Test** | Button | Send a test notification to verify configuration |

#### 4.5.3 LINE Notification

- **Status**: Service reached End of Life (EOL) — **UI removed entirely** from the Notification tab.
- **Backward compatibility**: Script API (`notify.line`) preserved for existing user scripts. No UI for configuration.

### 4.6 Others Tab

#### 4.6.1 Settings Groups

| Section | Controls | Type |
|---------|----------|------|
| **Output Size Adjuster** | Slider (0–100) to control width ratio between Output#1 and Output#2 | Scale/Slider |
| **Stdout Destination** | Output#1 / Output#2 radio buttons selecting where stdout prints go | Radio button |
| **Clear Outputs** | Button to clear both output panels | Button |
| **Widget Mode** | Combobox with 7 modes (see Section 3) | Combobox |
| **Software-Controller Position** | TOP / BOTTOM radio buttons for location within right panel | Radio button |
| **Dialogue Button Position** | TOP / BOTTOM / BOTH radio buttons for dialogue button placement | Radio button |

#### 4.6.2 Future / Phase 7 Items (Low Priority but Mandatory)

- Key configuration editor (advanced key binding UI).
- Pokémon Home integration (details unknown — reserved section).

---

## 5. Communication Protocol

### 5.1 Stack Overview

```
Camera Video:     WebRTC video track ──→ MJPEG over HTTP fallback
Controller Input: WebRTC DataChannel ──→ WebSocket fallback
Logs/Events:      WebRTC DataChannel ──→ WebSocket fallback
API Calls:        HTTP REST (axum)     ──→ (no fallback needed)
```

### 5.2 WebRTC (Primary)

- **Video**: WebRTC `RTCPeerConnection` with video track.
- **DataChannel**: For controller input events and log streaming.
- **Signaling**: HTTP-based SDP exchange.
- **Auto-reconnect**: On connection loss, retry every 3 seconds.

### 5.3 WebSocket (Fallback)

- **Endpoint**: `/ws`.
- **Messages**: JSON format.
- **Auto-reconnect**: On connection loss, retry every 3 seconds.
- **Events**:

| Event | Direction | Payload |
|-------|-----------|---------|
| `camera.frame` | Server → Client | Base64-encoded JPEG frame data |
| `command.start` | Server → Client | Command execution start notification |
| `command.stop` | Server → Client | Command execution stop notification |
| `command.error` | Server → Client | Command execution error details |
| `serial.data` | Server → Client | Serial port incoming data |
| `ping` | Bidirectional | Keepalive ping |
| `pong` | Bidirectional | Keepalive pong response |

### 5.4 HTTP REST API

- **Framework**: axum (Rust backend).
- **Documentation**: utoipa v5 with OpenAPI specification.
- **Code generation**: `openapi-typescript` for TypeScript client types.
- **Authentication**: None (local/LAN use only).
- **Modules**: ~9 modules with ~39 endpoints total.
- **Response format**: JSON with consistent structure.

### 5.5 Keyboard Input API

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/controller/keyboard` | Get current keyboard configuration |
| POST | `/api/controller/keyboard` | Set keyboard configuration |

- **Shortcuts**: F5 = Reload, F6 = Start, ESC = Stop.
- **Storage**: Keyboard settings stored in `localStorage`.

### 5.6 Mouse Input API

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/controller/mouse_stick?stick=LSTICK|RSTICK` | Get mouse stick configuration |
| POST | `/api/controller/mouse_stick` | Set mouse stick configuration (stick, enabled, sensitivity) |
| POST | `/api/input/stick` | Send stick input (`{x: 0–255, y: 0–255}`) |

### 5.7 Gamepad Input API

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/controller/type` | Get current gamepad type configuration |
| POST | `/api/controller/type` | Set gamepad type (`gamepad_type: "ProController" | "Xinput"`) |

- **Supported buttons**: A, B, X, Y, UP, DOWN, LEFT, RIGHT, L, R, ZL, ZR, MINUS, PLUS, HOME, CAPTURE.
- **Analog sticks**: 0–255 range for both axes.
- **Touchpad**: `{x: 0–320, y: 0–240}` coordinates.

---

## 6. Type System

### 6.1 OpenAPI → TypeScript

- **Source**: Rust backend with `utoipa` v5 macros.
- **Generation**: `openapi-typescript` CLI.
- **Output**: `Docs/api/openapi.ts`.
- **Usage**: All API calls and WebSocket messages must use generated types.

### 6.2 Type Safety Requirements

- Strict TypeScript (`strict: true`).
- No `any` types for API-related code.
- Runtime validation with Zod or similar for external inputs.

---

## 7. PWA Requirements

> **Status: NOT IMPLEMENTED — FUTURE PHASE ONLY.**  
> The following requirements are recorded as user requests for future implementation. They are not in current scope.

### 7.1 Manifest

- `manifest.json` with app metadata.
- Icons for all platforms.
- Display mode: `standalone`.

### 7.2 Service Worker

- Offline capability for UI assets.
- Background sync for queued commands (future).

### 7.3 Install Prompt

- Custom install button.
- Platform-specific install guidance.

---

## 8. Theme Support

> **Status: NOT IMPLEMENTED — FUTURE PHASE ONLY.**  
> Tailwind CSS v4 is confirmed as the styling framework. Theme system requirements recorded below.

### 8.1 Built-in Themes

- Light theme.
- Dark theme.
- System preference auto-detect.

### 8.2 Custom Themes (Future)

- User-defined color schemes.
- CSS variable-based theming.

---

## 9. Configuration System

### 9.1 Settings File

> **IMPORTANT**: `settings.ini` is **abolished**. The legacy INI-based configuration is replaced by Rust-native configuration management. Exact format and storage location are determined by the Rust backend team (out of scope for this UI spec).

### 9.2 Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `POKECON_DISABLE_COMPOSITING` | Disable compositing mode (Tauri) | `0` |
| `POKECON_WEB_DIR` | Static file directory | `web/dist` |
| `POKECON_PORT` | HTTP server port | `8020` |

### 9.3 Client-Side Storage

| Item | Storage Method | Notes |
|------|---------------|-------|
| Shortcut button assignments | `localStorage` | 10 button-key bindings |
| Keyboard settings | `localStorage` | Key mapping configuration |

---

## 10. Implementation Phases (from PLAN.md)

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Project setup, CI/CD, Nix flake | ✅ Complete |
| 1 | SvelteKit scaffold, remove React | ✅ Complete |
| 2 | API/OpenAPI integration | In Progress |
| 3 | TypeScript CI, linting, testing | In Progress |
| 4 | Component reimplementation | Pending |
| 5 | Camera streaming (WebRTC/MJPEG) | Pending |
| 6 | Build integration, Tauri config | Pending |
| 7 | Low-priority features (key config, Pokémon Home) | Pending |
| 8 | PWA support | Pending |
| 9 | Theme support (Tailwind v4) | Pending |

### 10.1 Process Requirements (User Mandates)

The following process requirements have been explicitly mandated by the user and must be followed for all phases:

| Requirement | Details |
|-------------|---------|
| **Execution via nix flake** | All development, testing, and builds must be run through `nix flake` commands. Direct execution of build tools is not permitted. |
| **opencode review breaks** | Every opencode review boundary must serve as a logical break point. Work should be structured to align with review boundaries. |
| **Commit and push per phase** | Each completed phase must be committed and pushed. No stacking of multiple phases in a single commit. |

---

## 11. Key User Requirements and Refusals

This section records explicit user mandates that overrule or clarify the specification.

### 11.1 React Code — Complete Removal

> **Requirement**: The existing React frontend codebase is considered "garbage" and must never be referenced, imported, or used as a source of truth. The SvelteKit implementation must derive its specification from the original Tkinter layout requirements communicated by the user, not from any React implementation.

### 11.2 Shortcut Buttons — 10 (Not 4)

> **Requirement**: The Commands tab must have exactly **10** shortcut buttons (not 4). This was explicitly changed from the original count.

### 11.3 Execution Controls — Start/Pause/Restart/Stop (Not Just Start/Stop)

> **Requirement**: The execution control buttons must include **Start, Pause, Restart, and Stop** (not just Start and Stop). Pause must be resumable.

### 11.4 LINE Notification — UI Removed

> **Requirement**: The LINE notification UI has been removed from the Notification tab due to service EOL. Script API (`notify.line`) is preserved for backward compatibility only, with no UI configuration.

### 11.5 Settings File — `settings.ini` Abolished

> **Requirement**: The legacy `settings.ini` file format is abolished. Rust-native configuration management replaces it. Exact format and implementation are out of scope for the UI specification.

### 11.6 PWA — Future Phase Only

> **Requirement**: PWA implementation is deferred to a future phase. Current scope does not include manifest generation, service workers, or install prompts.

### 11.8 Script Compatibility — All Pre-Refactoring Scripts Must Work

> **Requirement**: All scripts that worked with the pre-refactoring (Tkinter/Python) version must continue to work normally. No breaking changes to the script API. The Python compatibility layer must maintain full backward compatibility.

> **Requirement**: Theme support (light/dark/custom) is deferred to a future phase. Tailwind CSS v4 is confirmed as the styling framework.

---

## 12. Non-Functional Requirements

### 12.1 Performance

| Metric | Target |
|--------|--------|
| Video latency (WebRTC) | < 100ms |
| Video latency (MJPEG fallback) | < 300ms |
| Controller input latency | < 50ms |
| UI responsiveness | 60fps animations, < 16ms input response |

### 12.2 Accessibility

- Keyboard navigation for all controls.
- ARIA labels for screen readers.
- High contrast mode support.

### 12.3 Browser Support

| Browser | Minimum Version |
|---------|----------------|
| Chrome/Edge | 90+ |
| Firefox | 88+ |
| Safari | 14+ |

### 12.4 WebSocket Auto-Reconnect

- On connection loss, automatically retry connection every 3 seconds.
- Must handle temporary server unavailability gracefully.

---

## 13. Appendix: Tkinter UI Reference

### 13.1 Original Tab Details

The original Python/Tkinter UI used `tkinter.ttk.Notebook` with the following structure:

- **CameraTab**: `cv2.VideoCapture` with threaded frame reader, PIL resize, `ImageTk.PhotoImage` canvas display. Canvas supports mouse-driven stick control, color picker, and region screenshot.
- **SerialTab**: COM port dropdown, baud rate selector (9600/115200), data format selector (Default/Qingpi/3DS Controller), connect button with status indicator, serial monitor with Text+Scrollbar.
- **ManualControlTab**: Software Control (Keyboard checkbox, LStick Mouse, RStick Mouse), Hardware Control (ProController/Xinput radio, Record checkbox), Switch Controller Simulator with full Joy-Con layout.
- **CommandTab**: 3 sub-tabs (Python Command, Mcu Command, Shortcut), file browser, tag filter dropdown, command list with Listbox/Treeview, 10 shortcut buttons, execution buttons (Start/Pause/Restart/Stop/Reload).
- **NotificationTab**: Discord webhook URL, username, avatar URL inputs with Test buttons. Windows notify start/end checkboxes with Test button. LINE UI (removed — service EOL).
- **OthersTab**: Output Size Adjuster slider, Stdout Destination radio (Output#1/Output#2), Clear Outputs button, Widget Mode combobox (7 modes), Software-Controller Position radio (TOP/BOTTOM), Dialogue Button Position radio (TOP/BOTTOM/BOTH).

### 13.2 Original Controller Layout

- **Software Controller**: Canvas-based Joy-Con drawing with `<Button-1>` event binding for hold, `<ButtonRelease-1>` for release, Shift+release for `holdEndSkip`.
- **Colors**: L-side cyan `#56CCF2`, R-side red `#E9514E`, active state yellow `#FFD800`.
- **Analog stick dead zone**: ±10% from center (values 103–153 treated as neutral on 0–255 scale).
- **Buttons**: A, B, X, Y, L, R, ZL, ZR, +, −, Home, Capture, D-pad (4 directions), L-stick, R-stick, Touch screen (320×240).

### 13.3 Original Output Panels

- **Output #1 and Output #2**: Log display with ratio adjustment via slider (0–100).
- **Source**: Logs received via WebSocket.
- **Clear**: "Clear Outputs" button in Others tab.
---

## 14. Python公開API仕様と開発環境設定

> **Version**: 2.1.0-draft  
> **Date**: 2026-05-27  
> **Scope**: Python互換レイヤー、PyO3バインディング、開発環境自動構築  
> **Source**: Grill-meセッション決定事項（refactor/rust-coreブランチ）

---

### 14.1 設計方針

- **コアはRust**: すべてのコア処理はRustで実装。Pythonは必要な部分のみ（ユーザースクリプトAPI、互換レイヤー）。
- **メタクラスによる切り替え**: `CommandMeta`が将来の実装切り替え用フックを提供。現状はすべてPyO3（Rustバインディング）に流れる。
- **後方互換性**: リファクタリング前のスクリプトは変更なしで動作する必要がある。
- **型ヒント**: 新APIは動作する型ヒントを持つ。旧APIは非推奨として保持される。

### 14.2 パッケージ構造

```
pokecon/
├── __init__.py          # パッケージエントリ、モジュールパッチ
├── commands.py          # PythonCommand、ImageProcPythonCommand、CommandEngine
├── keys.py              # Button、Hat、Direction、Stick、Touchscreen、SendFormat
├── events.py            # EventBus（動的設定用）
├── dialogue.py          # ダイアログ関数（ブロッキングWebポップアップ）
├── _meta.py             # CommandMetaメタクラス
├── _adapter.py          # Rustコアアダプタ
├── cli_args.py          # CLI引数解析
├── script_loader.py     # スクリプト検出と読み込み
└── scripts_dir.py       # XDG準拠スクリプトディレクトリユーティリティ
```

PyO3モジュール（rust/pokecon-pybindings）:
- `pokecon.keys` — 入力型（Button、Hat、Direction、Stick、Touchscreen）
- `pokecon.command` — コマンドスキャン/読み込み
- `pokecon.events` — イベントバス
- `pokecon.notify` — 通知（Discord、LINEスタブ、Windows）
- `pokecon.sender` — シリアル通信
- `pokecon.dialogue` — ダイアログ関数
- `pokecon.image_proc` — 画像処理（opencv-rust）
- `pokecon.net` — Socket、MQTT、HTTPクライアント

### 14.3 コマンドクラス

#### 14.3.1 クラス階層

```
Command (ABC, metaclass=CommandMeta)
├── PythonCommand (ABC)
│   └── ImageProcPythonCommand (ABC)
└── McuCommandBase
```

#### 14.3.2 PythonCommand

**Import**: `from Commands.PythonCommandBase import PythonCommand`

**ライフサイクルメソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `do()` | `do() -> None` | 抽象 — 自動化ロジックをオーバーライド |
| `finish()` | `finish() -> None` | スクリプトを正常停止 |
| `checkIfAlive()` | `checkIfAlive() -> Literal[True]` | 停止フラグ確認；終了時は`StopThread`を送出 |

**入力メソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `press()` | `press(buttons, duration=0.1, wait=0.1)` | ボタンをduration秒押下後解放、wait秒待機 |
| `pressRep()` | `pressRep(buttons, repeat, duration=0.1, interval=0.1, wait=0.1)` | 繰り返し押下 |
| `hold()` | `hold(buttons, wait=0.1)` | ボタンを押下状態で保持 |
| `holdEnd()` | `holdEnd(buttons)` | 保持中のボタンを解放 |
| `wait()` | `wait(wait: float)` | wait秒スリープ |
| `short_wait()` | `short_wait(wait: float)` | ビジーループ待機（高精度） |
| `direct_serial()` | `direct_serial(commands, waittimes)` | 生シリアルコマンド送信 |
| `reload_com_port()` | `reload_com_port()` | COMポート接続を再読み込み |

**出力メソッド**（PyO3実装）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `print_t1()` | `print_t1(*objects, sep=' ', end='\n')` | 上部ログパネルへ出力 |
| `print_t2()` | `print_t2(*objects, sep=' ', end='\n')` | 下部ログパネルへ出力 |
| `print_t()` | `print_t(*objects, sep=' ', end='\n')` | stdout以外のログパネルへ出力 |
| `print_s()` | `print_s(*objects, sep=' ', end='\n')` | stdout割り当てパネルへ出力 |
| `print_ts()` | `print_ts(*objects, sep=' ', end='\n')` | `print_s`と同じ |
| `print_t1b()` | `print_t1b(mode, *objects, sep=' ', end='\n')` | 上部ログ（モード付き w/a/d） |
| `print_t2b()` | `print_t2b(mode, *objects, sep=' ', end='\n')` | 下部ログ（モード付き） |
| `print_tb()` | `print_tb(mode, *objects, sep=' ', end='\n')` | stdout以外ログ（モード付き） |
| `print_tbs()` | `print_tbs(mode, *objects, sep=' ', end='\n')` | stdoutログ（モード付き） |

**ダイアログメソッド**（ブロッキングWebポップアップ）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `dialogue()` | `dialogue(title, message, desc=None, need=list)` | 単純入力ダイアログ（非推奨、show_dialog使用） |
| `dialogue6widget()` | `dialogue6widget(title, dialogue_list, desc=None, need=list)` | マルチウィジェットダイアログ（非推奨、show_dialog使用） |

**Socketメソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `socket_connect()` | `socket_connect()` | Socketサーバへ接続 |
| `socket_disconnect()` | `socket_disconnect()` | Socketサーバから切断 |
| `socket_transmit_message()` | `socket_transmit_message(message)` | Socket経由でメッセージ送信 |
| `socket_receive_message()` | `socket_receive_message(header, show_msg=False)` | ヘッダーフィルタ付き受信 |
| `socket_receive_message2()` | `socket_receive_message2(headerlist, show_msg=False)` | 複数ヘッダーフィルタ付き受信 |
| `socket_change_ipaddr()` | `socket_change_ipaddr(addr)` | Socket IPアドレス変更 |
| `socket_change_port()` | `socket_change_port(port)` | Socketポート変更 |
| `socket_change_alive()` | `socket_change_alive(flag)` | Socket aliveフラグ設定 |

**MQTTメソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `mqtt_transmit_message()` | `mqtt_transmit_message(roomid, message)` | MQTTトピックへメッセージ公開 |
| `mqtt_receive_message()` | `mqtt_receive_message(roomid, header, show_msg=False)` | ヘッダーフィルタ付き購読/受信 |
| `mqtt_receive_message2()` | `mqtt_receive_message2(roomid, headerlist, show_msg=False)` | 複数ヘッダーフィルタ付き購読 |
| `mqtt_change_broker_address()` | `mqtt_change_broker_address(broker_address)` | MQTTブローカーアドレス変更 |
| `mqtt_change_id()` | `mqtt_change_id(mqtt_id)` | MQTTクライアントID変更 |
| `mqtt_change_clientId()` | `mqtt_change_clientId(clientId)` | MQTT接続名変更 |
| `mqtt_change_pub_token()` | `mqtt_change_pub_token(pub_token)` | 公開トークン変更 |
| `mqtt_change_sub_token()` | `mqtt_change_sub_token(sub_token)` | 購読トークン変更 |

**通知メソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `discord_text()` | `discord_text(content='', index=0, keys='DISCORD_WEBHOOK')` | Discord webhook経由でテスト送信 |
| `discord_image()` | `discord_image(content='', index=0, crop_fmt='', crop=None, keys='DISCORD_WEBHOOK')` | Discord webhook経由でテキスト+スクリーンショット送信 |
| `LINE_text()` | `LINE_text(txt, token='')` | No-opスタブ（LINEサービスEOL） |
| `LINE_image()` | `LINE_image(txt, crop_fmt='', crop=None, token='')` | No-opスタブ（LINEサービスEOL） |
| `win_notification()` | `win_notification()` | Windowsデスクトップトースト通知 |

#### 14.3.3 ImageProcPythonCommand

**Import**: `from Commands.PythonCommandBase import ImageProcPythonCommand`

`PythonCommand`を拡張し、カメラと画像処理機能を追加。

**コンストラクタ**: `ImageProcPythonCommand(cam, gui=None)`

**画像処理メソッド**（Rust opencv-rust実装）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `isContainTemplate()` | `isContainTemplate(template_path, threshold=0.7, use_gray=True, ...)` | カメラフレームに対するテンプレートマッチング |
| `isContainTemplate_max()` | `isContainTemplate_max(template_path_list, threshold=0.7, ...)` | マルチテンプレートマッチング |
| `isContainTemplateGPU()` | `isContainTemplateGPU(template_path, threshold=0.7, ...)` | GPU高速テンプレートマッチング |
| `isContainedImage()` | `isContainedImage(image_path, threshold=0.7, ...)` | 逆テンプレートマッチング |
| `saveCapture()` | `saveCapture(filename=None, crop_fmt='', crop=None, mode=True)` | カメラフレームを./Captures/へ保存 |
| `popupImage()` | `popupImage(crop_fmt='', crop=None, title='image')` | カメラフレームをポップアップ表示 |
| `getCameraImage()` | `getCameraImage(crop_fmt='', crop=None)` | カメラフレームをOpenCV画像配列で取得 |
| `openImage()` | `openImage(filename, mode='t')` | 画像ファイルを読み込み |
| `setTemplateDir()` | `setTemplateDir(path)` | テンプレート画像ディレクトリを変更 |
| `get_filespec()` | `get_filespec(filename, mode='t')` | 相対ファイル名をフルパスに解決 |
| `displayRectangle()` | `displayRectangle(max_loc, width, height, tag=None, ms=2000, color=None, crop_fmt='', crop=None)` | GUIキャンバスオーバーレイに矩形描画 |
| `displayText()` | `displayText(position, txt, tag=None, ms=2000, font='UD デジタル 教科書体 NP-B', fontsize=20, color='black')` | GUIキャンバスオーバーレイにテキスト描画 |

**内部関数**（互換レイヤー用に`_`プレフィックスで公開）:
| 関数 | シグネチャ | 説明 |
|------|-----------|-------------|
| `_template_match()` | `_template_match(image, template, threshold, use_gray, ...)` | コアテンプレートマッチング |
| `_grayscale()` | `_grayscale(image)` | グレースケール変換 |
| `_resize()` | `_resize(image, width, height)` | 画像リサイズ |

#### 14.3.4 McuCommandBase

**Import**: `from Commands.McuCommandBase import McuCommandBase`

ファームウェアベースコマンド用。PythonCommandと同じメタクラス切り替え。

### 14.4 メタクラス設計（CommandMeta）

```python
class CommandMeta(type):
    """実装切り替え用メタクラス。
    
    現状はすべての実装がPyO3（Rustバインディング）に流れる。
    将来: クラス変数や関数使用パターンに基づいて切り替え。
    """
    def __call__(cls, *args, **kwargs):
        # 将来: cls.__target_implementation__等をチェック
        # 現状: 常にPyO3実装を使用
        return super().__call__(*args, **kwargs)
```

### 14.5 KeyPressとSender

#### 14.5.1 KeyPress

- ユーザースクリプトに**直接公開されない**
- `self.keys.neutral()`のみアクセス可能（コントローラーをニュートラル状態にリセット）
- 内部実装はRust、PyO3経由で公開

#### 14.5.2 Sender

**PyO3実装**（限定公開API）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `writeRow()` | `writeRow(row: str)` | シリアル行を書き込み |
| `ser.write()` | `ser.write(data)` | 直接シリアル書き込み（PyO3でpySerial互換型変換） |

その他のSenderメソッドはSenderクラスとして公開されず、適切な他クラスに統合。

### 14.6 新ダイアログAPI（型安全）

**非推奨**: `dialogue()`、`dialogue6widget()` — 互換性のために保持、非推奨マーク。

**新API**: `show_dialog()` with Widgetクラスと型ヒント。

```python
from typing import Generic, TypeVar, overload, Literal

T = TypeVar('T')

class Widget(Generic[T]):
    @overload
    def __init__(self: "Widget[str]", widget_type: Literal["Entry"], label: str, default: str) -> None: ...
    
    @overload
    def __init__(self: "Widget[bool]", widget_type: Literal["Check"], label: str, default: bool) -> None: ...
    
    @overload
    def __init__(self: "Widget[T]", widget_type: Literal["Combo"], label: str, options: list[T], default: T) -> None: ...
    
    @overload
    def __init__(self: "Widget[int]", widget_type: Literal["Spin"], label: str, options: list[int], default: int) -> None: ...
    
    def __init__(self, widget_type, label, *args, **kwargs) -> None:
        self.widget_type = widget_type
        self.label = label
        self.value: T | None = None  # ダイアログ後に結果を格納

# 使用例
entry = Widget("Entry", "名前", "デフォルト")  # Widget[str]
check = Widget("Check", "有効", True)  # Widget[bool]
combo = Widget("Combo", "選択肢", ["A", "B", "C"], "A")  # Widget[str]
spin = Widget("Spin", "数値", [1, 2, 3], 1)  # Widget[int]

show_dialog("タイトル", widgets=[entry, check, combo, spin])

print(entry.value)  # str
print(check.value)  # bool
print(combo.value)  # str
print(spin.value)  # int
```

### 14.7 イベントシステム（動的設定）

動的設定ファイル（PythonおよびLua）で使用。

```python
# Python設定
import pokecon

pokecon.autocmd.create("camera.open", callback=lambda: print("Camera opened"))
pokecon.autocmd.create("serial.connect", pattern="COM3", callback=lambda: print("Connected"))
```

```lua
-- Lua設定（Neovim風require-less）
pokecon.autocmd.create("camera.open", {
    callback = function()
        print("Camera opened")
    end
})
```

### 14.8 設定ファイルシステム

#### 14.8.1 Python設定

```python
# config.py
import pokecon

pokecon.opt.camera.fps = 60
pokecon.opt.camera.resolution = "1280x720"
pokecon.opt.serial.port = "COM3"
pokecon.opt.serial.baudrate = 115200

pokecon.keymap.set("controller", "A", lambda: pokecon.input.press(pokecon.keys.Button.A))
```

#### 14.8.2 Lua設定

```lua
-- config.lua
pokecon.opt.camera.fps = 60
pokecon.opt.serial.port = "COM3"

pokecon.autocmd.create("camera.open", {
    callback = function()
        print("Camera opened")
    end
})
```

### 14.9 スクリプト互換性要件

| 要件 | 状態 |
|------|------|
| 17以上のサンプルスクリプトが変更なしで動作 | ✅ 必須 |
| `from Commands.PythonCommandBase import PythonCommand` | ✅ モジュールパッチで保持 |
| `from Commands.Keys import Button, Hat, ...` | ✅ モジュールパッチで保持 |
| `self.keys.neutral()` | ✅ 利用可能 |
| `self.keys.ser.writeRow()` | ✅ 利用可能 |
| `self.keys.ser.ser.write()` | ✅ 利用可能（Rustシリアルラッパー） |
| 画像処理API | ✅ Rust実装（opencv-rust） |
| Discord通知 | ✅ 実装済み |
| LINE通知 | ⚠️ No-opスタブ（サービスEOL） |
| Windows通知 | ✅ 実装済み |

### 14.10 開発環境自動構築

#### 14.10.1 ディレクトリ構造

```
~/.config/pokecon/                    # XDG_CONFIG_HOME（ユーザーが編集する）
├── pyproject.toml                    # Python LSP設定
├── .luarc.json                       # Lua LSP設定（lua-language-server & EmmyLua共用）
├── .vscode/settings.json             # Pylance用（オプション）
├── settings.toml                     # ユーザー設定
│   [python.packages]                 # ユーザー追加ライブラリ
├── init.py                           # Python動的設定テンプレート
└── init.lua                          # Lua動的設定テンプレート

~/.local/share/pokecon/               # XDG_DATA_HOME（自動管理）
├── typings/                          # Python型定義（.pyi、Rust側で自動生成）
├── lua-typings/                      # Lua型定義（.d.lua、Rust側で自動生成）
├── venv/                             # Python仮想環境
└── python/                           # python-build-standalone（非nix環境）
```

#### 14.10.2 設定ファイル生成タイミング

- **存在しない時に生成**（初回、アップデート、削除後等）
- **nix環境**: nix式で指定した場合のみnix側で生成。指定しなかった場合はアプリ起動時に存在しないためアプリ側で生成。
- **非nix環境**: アプリ側で自動生成

#### 14.10.3 Python管理（非nix環境）

```rust
// Rust側
struct PythonManager {
    data_dir: PathBuf,           // ~/.local/share/pokecon/
    expected_python_version: Option<String>, // オプション（デフォルト推奨）
}

impl PythonManager {
    fn ensure_python(&self) -> PathBuf {
        // 1. 期待するバージョンがない場合はデフォルトを使用
        // 2. 既存のPythonが期待するバージョンかチェック
        // 3. ない場合はastral-sh/python-build-standaloneをダウンロード
        // 4. 仮想環境を構築
        // 5. 必須パッケージ + ユーザーパッケージをインストール
    }
}
```

#### 14.10.4 必須パッケージ管理

- **リポジトリ内`pyproject.toml`**からビルド時に取得
- **`build.rs`で`OUT_DIR`にコード生成**、`include!`で埋め込み
- `cargo:rerun-if-changed=../pyproject.toml`で再ビルドトリガー

```rust
// build.rs
fn main() {
    println!("cargo:rerun-if-changed=../pyproject.toml");
    // pyproject.tomlを読み込み、依存関係をパース
    // 生成コードをOUT_DIRに書き出し
}
```

#### 14.10.5 ユーザーパッケージ設定

```toml
# ~/.config/pokecon/settings.toml
# ユーザーが触る設定ファイル（必須パッケージは含まない）

[python]
# Pythonバージョン（オプション、デフォルト推奨）
# version = "3.12"  # 非推奨: 基本的にはデフォルトを使用

# ユーザー追加ライブラリ
[[python.packages]]
name = "requests"
version = ">=2.28.0"  # バージョン指定あり

[[python.packages]]
name = "numpy"        # バージョン指定なし（最新版）

[[python.packages]]
name = "custom-lib"
version = "1.0.0"
source = "git+https://github.com/user/custom-lib.git"  # 取得元指定

[[python.packages]]
name = "local-lib"
version = "0.5.0"
source = "path=/home/user/projects/local-lib"  # ローカルパス
```

#### 14.10.6 LSP設定（pyproject.toml）

```toml
[tool.basedpyright]
extraPaths = ["/home/username/.local/share/pokecon/typings"]
venvPath = "/home/username/.local/share/pokecon"
venv = "venv"

[tool.pyright]
extraPaths = ["/home/username/.local/share/pokecon/typings"]
venvPath = "/home/username/.local/share/pokecon"
venv = "venv"

[tool.mypy]
mypy_path = ["/home/username/.local/share/pokecon/typings"]

[tool.pylsp.plugins.jedi]
extra_paths = ["/home/username/.local/share/pokecon/typings"]

[tool.pyrefly]
search_path = ["/home/username/.local/share/pokecon/typings"]

[tool.ty.environment]
extra-paths = ["/home/username/.local/share/pokecon/typings"]
python = "/home/username/.local/share/pokecon/venv/bin/python"

[tool.ruff]
# ruffはextraPaths未対応（LSP機能限定）
```

#### 14.10.7 Lua LSP設定（.luarc.json）

```json
{
    "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
    "workspace.library": [
        "/home/username/.local/share/pokecon/lua-typings"
    ]
}
```

---

*This specification is a living document. Updates should be made when new requirements are communicated by the user.*
