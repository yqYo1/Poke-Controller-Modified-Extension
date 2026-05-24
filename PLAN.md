# Poke-Controller Modified Extension — UI Refactoring Implementation Plan

> **Version**: 2.0.0-draft  
> **Branch**: `refactor/rust-core`  
> **Date**: 2026-05-24  
> **Scope**: Web/Desktop UI (SvelteKit) — Backend API and Rust core are out of scope  
> **Source**: SPECIFICATION.md compliance implementation

---

## 1. Overview

This document describes the implementation plan to bring the current SvelteKit UI into full compliance with SPECIFICATION.md. The specification is derived exclusively from past user requirements communicated during refactoring sessions, not from the current codebase.

### 1.1 Design Philosophy

- **Visual parity with Tkinter**: The new UI must closely match the original Tkinter layout and appearance, not merely replicate features functionally.
- **Script compatibility**: All scripts that worked with the pre-refactoring version must continue to work normally.
- **Modern stack**: SvelteKit + Svelte 5 (runes mode) + Tailwind CSS v4.
- **Low-latency communication**: WebRTC primary with HTTP/MJPEG and WebSocket fallbacks.
- **Type safety**: OpenAPI-generated TypeScript types from Rust backend.

---

## 2. Current Implementation Status

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Project setup, CI/CD, Nix flake | ✅ Complete |
| 1 | SvelteKit scaffold, remove React | ✅ Complete |
| 2 | API/OpenAPI integration | ✅ Complete |
| 3 | TypeScript CI, linting, testing | 🔄 Partial |
| 4 | Component reimplementation | ✅ Complete |
| 5 | Camera streaming (WebRTC/MJPEG) | ✅ Complete |
| 6 | Build integration, Tauri config | ✅ Complete |
| 7 | Low-priority features (key config, Pokémon Home) | ✅ Complete |
| 8 | PWA support | ✅ Complete |
| 9 | Theme support (Tailwind v4) | ✅ Complete |

---

## 3. Implementation Phases

### Phase 1: Core Layout and Widget Mode System

**Priority**: P0 (Critical)  
**Goal**: Implement the 7 widget modes that control visibility of right panel components.

#### 3.1.1 Widget Mode State Management

- Create a centralized widget mode store (`$lib/stores/widget.ts`)
- Implement 7 modes as defined in SPEC §3:
  - Mode 1: Show all (Software Controller + Output#1 + Output#2) — default
  - Mode 2: Software Controller + Output#1
  - Mode 3: Software Controller + Output#2
  - Mode 4: Output#1 + Output#2
  - Mode 5: Software Controller only
  - Mode 6: Output#1 only
  - Mode 7: Output#2 only

#### 3.1.2 RightPanel Dynamic Visibility

- Modify `RightPanel.svelte` to respond to widget mode changes
- Each section (SoftwareController, Output#1, Output#2) must be independently show/hide
- Respect Software-Controller Position setting (TOP/BOTTOM)

#### 3.1.3 Software-Controller Position

- Implement position toggle within RightPanel
- TOP: SoftwareController appears above Output panels
- BOTTOM: SoftwareController appears below Output panels (default)

**Caution Points**:
- Widget mode changes must be reactive and immediate
- Output panel sizing must adapt when sections are hidden/shown
- Store state must persist across tab switches

---

### Phase 2: Software Controller Full Implementation

**Priority**: P0 (Critical)  
**Goal**: Complete Joy-Con layout with correct colors and input handling.

#### 3.2.1 Visual Design

- Joy-Con L side: Cyan `#56CCF2`
- Joy-Con R side: Red `#E9514E`
- Active state (button pressed): Yellow `#FFD800`
- Match Tkinter canvas-based appearance as closely as possible

#### 3.2.2 Input Handling

- **Hold**: `<Button-1>` press triggers button hold (sends press signal)
- **Release**: `<ButtonRelease-1>` triggers button release (sends release signal)
- **Shift+Release**: Triggers `holdEndSkip` — only toggles visual state without sending release signal

#### 3.2.3 Button Layout

All standard Switch controller buttons:
- A, B, X, Y (face buttons)
- L, R, ZL, ZR (shoulder buttons)
- MINUS (−), PLUS (+)
- HOME, CAPTURE
- D-pad (Up, Down, Left, Right)
- L-stick (analog, 0–255 coordinates)
- R-stick (analog, 0–255 coordinates)
- Touch screen simulation (320×240 coordinate input)

#### 3.2.4 Analog Sticks

- 0–255 range on both X and Y axes
- Center position dead zone: ±10% (values 103–153 treated as neutral)
- Visual feedback for stick position

**Caution Points**:
- Color values are exact hex codes from SPEC — do not approximate with Tailwind defaults
- Input events must use proper mouse event handlers (mousedown/mouseup/mouseleave)
- Shift+click behavior is subtle — must not send release signal to backend
- Analog stick dead zone must be implemented in both visual and API layers

---

### Phase 3: Camera Tab

**Priority**: P0 (Critical)  
**Goal**: Full camera functionality with WebRTC/MJPEG support.

#### 3.3.1 Video Feed Display

- Canvas element for video rendering (not `<img>` or `<video>` for primary display)
- Primary: WebRTC video track via `WebRTCVideoClient`
- Fallback: MJPEG over HTTP
- Frame rate: Configurable via FPS setting

#### 3.3.2 Camera Settings

| Control | Type | Description |
|---------|------|-------------|
| Camera device selection | Combobox | Dropdown of available camera devices |
| FPS | Combobox | Configurable frames per second (1–30fps) |
| Flip | Checkbox | Horizontal/vertical flip toggle |

#### 3.3.3 Display Mode Toggles

- Realtime checkbox
- Value checkbox
- Guide checkbox

#### 3.3.4 Mouse Operations on Canvas

| Action | Trigger | Behavior |
|--------|---------|----------|
| LStick/RStick control | Mouse drag on canvas | Emulates analog stick movement |
| Color picker | Ctrl+Click | Picks color value at click position |
| Range screenshot | Ctrl+Shift+Drag | Captures rectangular region |
| Named save | Ctrl+Alt+Drag | Saves to named file prompt |

#### 3.3.5 Screenshot Capture

- Save location: `./Captures/` directory
- Format: PNG/JPEG selectable

**Caution Points**:
- WebRTC and MJPEG fallback must be seamless
- Canvas rendering must handle high-frequency frame updates efficiently
- Mouse drag operations must not conflict with normal UI interactions
- Screenshot functionality requires backend API integration

---

### Phase 4: Serial Tab

**Priority**: P0 (Critical)  
**Goal**: Serial port communication with monitor.

#### 3.4.1 Connection Control

| Control | Type | Description |
|---------|------|-------------|
| COM port selection | Combobox | Dropdown of available ports |
| Refresh button | Button | Rescan available ports |
| Connect/Disconnect | Toggle button | Connect/disconnect |

#### 3.4.2 Configuration

| Setting | Options | Default |
|---------|---------|---------|
| Baud Rate | 9600 / 115200 | 9600 |
| Data Format | Default / Qingpi / 3DS Controller | Default |

#### 3.4.3 Serial Monitor

- Text widget with scrollbar
- Real-time incoming/outgoing data display
- Auto-scroll to latest entry
- Clear button

**Caution Points**:
- Port list must refresh dynamically
- Serial monitor must handle high-frequency data without UI locking
- Data format selection affects baud rate (Default/Qingpi = 9600, 3DS = 115200)

---

### Phase 5: Manual Control Tab

**Priority**: P0 (Critical)  
**Goal**: Software and hardware control sections.

#### 3.5.1 Software Control Section

| Control | Type | Description |
|---------|------|-------------|
| Keyboard | Checkbox | Enables keyboard-based controller input |
| LStick Mouse | Checkbox | Enables mouse emulation of left analog stick |
| RStick Mouse | Checkbox | Enables mouse emulation of right analog stick |

#### 3.5.2 Hardware Control Section

| Control | Type | Description |
|---------|------|-------------|
| ProController | Radio | Switch Pro Controller input mode |
| Xinput | Radio | Xbox-compatible controller input mode |
| Record | Checkbox | Enables input recording |

#### 3.5.3 Switch Controller Simulator

- Full Joy-Con style button layout in tab content area
- Same buttons as Software Controller (Phase 2)
- D-Pad, L/ZL, MINUS/CAPTURE, A/B/X/Y, R/ZR, PLUS/HOME, LSTICK, RSTICK, Touch screen

**Caution Points**:
- Hardware control requires gamepad API integration
- Recording functionality needs backend support
- Controller simulator in this tab is separate from the right panel Software Controller

---

### Phase 6: Commands Tab

**Priority**: P0 (Critical)  
**Goal**: Command management with 10 shortcut buttons.

#### 3.6.1 Sub-tab Structure

| Sub-tab | Content |
|---------|---------|
| Python Command | List/tree of available Python command scripts |
| Mcu Command | List/tree of available MCU command scripts |
| Shortcut | 10 shortcut button assignment grid |

#### 3.6.2 Command List

- Listbox or Treeview showing available commands
- Tag filter dropdown
- Columns: Command name, tags, description

#### 3.6.3 Shortcut Buttons (10 Buttons)

- Count: 10 (requirement changed from original 4)
- Assignment: User-assignable to any loaded command
- Shift+Click to assign
- Right-click to clear assignment
- Button label shows assigned command name
- Keyboard shortcuts: F1–F10
- Storage: `localStorage`

#### 3.6.4 Execution Control

| Button | Keyboard Shortcut | Action |
|--------|-------------------|--------|
| Start | F5 | Begin execution |
| Pause | Shift+F6 | Pause (resumable) |
| Restart | — | Restart from beginning |
| Stop | Escape | Abort immediately |
| Reload | — | Reload command list |

- Status display: Running / Paused / Stopped / Error
- Progress bar for supported commands

**Caution Points**:
- 10 shortcut buttons is a hard requirement (not 4)
- Start/Pause/Restart/Stop/Reload all required (not just Start/Stop)
- Shortcut assignments must persist in localStorage
- Keyboard shortcuts (F5, Shift+F6, Escape) must work globally

---

### Phase 7: Notification Tab

**Priority**: P1 (High)  
**Goal**: Windows and Discord notification settings.

#### 3.7.1 Windows Notification

| Control | Type | Description |
|---------|------|-------------|
| Notify on script start | Checkbox | Send notification when execution starts |
| Notify on script end | Checkbox | Send notification when execution ends |
| Test | Button | Send test notification |

#### 3.7.2 Discord Notification

| Control | Type | Description |
|---------|------|-------------|
| Webhook URL | Text input | Discord webhook URL with validation |
| Username | Text input | Custom username (optional) |
| Avatar URL | Text input | Custom avatar image URL (optional) |
| Test | Button | Send test notification |

#### 3.7.3 LINE Notification

- **UI removed entirely** (service EOL)
- Script API (`notify.line`) preserved for backward compatibility

**Caution Points**:
- LINE UI must be completely absent — no placeholder, no disabled field
- Discord webhook URL should be validated (basic format check)
- Test buttons must provide user feedback (success/error)

---

### Phase 8: Others Tab

**Priority**: P1 (High)  
**Goal**: Output and widget configuration.

#### 3.8.1 Settings Groups

| Section | Controls | Type |
|---------|----------|------|
| Output Size Adjuster | Slider (0–100) for Output#1/Output#2 width ratio | Scale/Slider |
| Stdout Destination | Output#1 / Output#2 radio buttons | Radio button |
| Clear Outputs | Button to clear both output panels | Button |
| Widget Mode | Combobox with 7 modes | Combobox |
| Software-Controller Position | TOP / BOTTOM radio buttons | Radio button |
| Dialogue Button Position | TOP / BOTTOM / BOTH radio buttons | Radio button |

**Caution Points**:
- Output Size Adjuster affects right panel layout in real-time
- Clear Outputs button must clear both panels simultaneously
- Widget Mode changes must be reflected immediately in right panel

---

### Phase 9: WebSocket Auto-Reconnect Adjustment

**Priority**: P1 (High)  
**Goal**: Align with SPEC requirements.

#### 3.9.1 Current Implementation

- Exponential backoff: 1s → 2s → 4s → ... → 30s max
- Max retries: 10
- Jitter: ±25%

#### 3.9.2 Required Change

- Fixed interval: 3 seconds
- No max retry limit (retry indefinitely)
- Remove exponential backoff

**Caution Points**:
- Current implementation uses exponential backoff — must be changed to fixed 3s interval
- WebSocket must auto-reconnect indefinitely, not stop after 10 retries

---

### Phase 10: Integration Testing and CI Verification

**Priority**: P0 (Critical)  
**Goal**: Ensure all changes pass CI and meet specification.

#### 3.10.1 Testing Checklist

- [ ] All 6 main tabs render correctly
- [ ] Widget Mode 1–7 all function correctly
- [ ] Software Controller shows correct colors and responds to input
- [ ] WebSocket auto-reconnects every 3 seconds
- [ ] Camera tab supports WebRTC and MJPEG fallback
- [ ] Serial tab can list ports and show monitor
- [ ] Commands tab has 10 shortcut buttons
- [ ] Start/Pause/Restart/Stop/Reload all present
- [ ] LINE UI is completely absent from Notification tab
- [ ] Output panels receive and display WebSocket logs

#### 3.10.2 CI Verification

- Run `nix run .#check` before every commit
- Run `nix run .#web-check` for frontend-specific checks
- Monitor GitHub Actions after push

---

## 4. Key User Requirements (Mandatory)

| Requirement | Details | Phase |
|-------------|---------|-------|
| React code fully removed | Never reference legacy React frontend | ✅ Done |
| Shortcut buttons — 10 (not 4) | Commands tab must have exactly 10 | Phase 6 |
| Execution controls — Start/Pause/Restart/Stop/Reload | Not just Start/Stop | Phase 6 |
| LINE Notification — UI removed | No LINE UI, script API preserved | Phase 7 |
| Settings file — `settings.ini` abolished | Rust-native config | Backend |
| Script compatibility | All pre-refactoring scripts must work | All phases |
| Visual parity with Tkinter | Match layout, colors, spacing | All phases |
| WebSocket auto-reconnect 3s | Fixed interval, not exponential | Phase 9 |
| Commit and push per phase | No stacking multiple phases | All phases |

---

## 5. Process Requirements

| Requirement | Details |
|-------------|---------|
| Execution via nix flake | All development via `nix run .#<task>` |
| opencode review breaks | Every review boundary is a logical break point |
| Commit and push per phase | Each phase committed and pushed separately |
| CI check before user review | Run `nix run .#check` AND opencode review before requesting review |

---

## 6. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| WebRTC implementation complexity | High | High | Implement MJPEG fallback first, then add WebRTC |
| Canvas performance with video | Medium | High | Use requestAnimationFrame, optimize draw calls |
| Tkinter visual parity difficult | Medium | Medium | Reference original screenshots, iterate with user |
| API endpoints missing in backend | Medium | High | Document gaps, implement UI with mock data |
| Widget mode state synchronization | Medium | Medium | Centralized store, reactive bindings |

---

*This plan is a living document. Updates should be made when new requirements are communicated by the user or when implementation reveals necessary adjustments.*
