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

*This specification is a living document. Updates should be made when new requirements are communicated by the user. Information marked as "Phase 7" or "Future Phase" is recorded for completeness but is not in current implementation scope.*
