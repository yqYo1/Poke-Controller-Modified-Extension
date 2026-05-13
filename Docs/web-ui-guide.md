# Web UI Guide — SvelteKit Edition

This document describes the **new SvelteKit-based Web UI** for Poke-Controller Modified
Extension.  The UI has been migrated from React 19 + Vite to SvelteKit 2 + Svelte 5,
with comprehensive TypeScript support, auto-generated OpenAPI types, and a modern
event-driven communication layer.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Project Structure](#2-project-structure)
3. [Development Workflow](#3-development-workflow)
4. [Build Instructions](#4-build-instructions)
5. [API Client Usage](#5-api-client-usage)
6. [Component Guidelines](#6-component-guidelines)
7. [Communication Protocols](#7-communication-protocols)
8. [Routing & Navigation](#8-routing--navigation)
9. [Testing](#9-testing)
10. [PWA & Service Worker](#10-pwa--service-worker)

---

## 1. Architecture Overview

### Tech Stack

| Layer | Technology |
|-------|-----------|
| Framework | SvelteKit 2 (SPA mode) |
| UI Runtime | Svelte 5 (runes mode) |
| Build Tool | Vite 6 |
| Styling | Tailwind CSS 3 |
| Type System | TypeScript 5 |
| Desktop Shell | Tauri v2 (optional wrapper) |
| HTTP Client | Fetch API + `openapi-typescript` |
| Real-time | WebSocket (with WebRTC DataChannel upgrade) |
| PWA | Service Worker + Web App Manifest |

### Key Design Decisions

- **SPA mode**: The app uses `@sveltejs/adapter-static` with a `fallback: 'index.html'`
  so it runs as a single-page application.  All routes are served under the `/ui/`
  base path; the Rust backend (axum) handles `/` → `/ui/` redirects.
- **Runes mode**: Svelte 5 runes (`$state`, `$derived`, `$effect`, `$props`) are
  enforced for all project source files (except `node_modules`).  This ensures
  consistent reactive patterns.
- **Tailwind CSS**: All components use utility-first styling via Tailwind with a
  custom dark theme.  The theme system exposes CSS custom properties for runtime
  switching (light/dark/custom).
- **OpenAPI codegen**: API types are auto-generated from the backend's OpenAPI spec
  using `openapi-typescript`.  Manual type re-exports bridge gaps where utoipa
  does not emit response schemas.

### Svelte 5 Runes — Quick Reference

```svelte
<script lang="ts">
  // ── Reactive state ──────────────────────────────────────────────────
  let count = $state(0);

  // ── Derived values ──────────────────────────────────────────────────
  let doubled = $derived(count * 2);

  // ── Side effects ────────────────────────────────────────────────────
  $effect(() => {
    console.log('count changed:', count);
  });

  // ── Component props ─────────────────────────────────────────────────
  let { name, version = '0.1.0' }: { name: string; version?: string } = $props();
</script>
```

---

## 2. Project Structure

```
web/
├── package.json                  # Dependencies, scripts
├── svelte.config.js              # SvelteKit config (adapter-static, /ui base)
├── vite.config.ts                # Vite config (proxy /api → 127.0.0.1:8020)
├── vitest.config.ts              # Vitest config (merges vite.config.ts)
├── tailwind.config.js            # Tailwind CSS config
├── postcss.config.js             # PostCSS config
├── eslint.config.js              # ESLint flat config
├── tsconfig.json                 # TypeScript config
│
├── src/
│   ├── app.html                  # HTML shell (PWA meta, manifest link)
│   ├── app.css                   # Global styles + Tailwind directives
│   ├── app.d.ts                  # Global ambient types
│   ├── service-worker.ts         # Service Worker (PWA offline support)
│   │
│   ├── routes/                   # SvelteKit file-based routing
│   │   ├── +layout.svelte        # Root layout: MenuBar + NavBar + StatusBar
│   │   ├── +layout.ts            # Layout load (CSR-only)
│   │   ├── +page.svelte          # Home page (status, links)
│   │   ├── camera/+page.svelte   # Camera tab — open/close, preview, capture
│   │   ├── serial/+page.svelte   # Serial tab — ports, connect, send
│   │   ├── manual/+page.svelte   # Manual Control tab — gamepad, mouse stick
│   │   ├── commands/+page.svelte # Commands tab — list, filter, start/stop
│   │   ├── keyconfig/+page.svelte# Key Config tab — keyboard bindings
│   │   ├── pokemonhome/+page.svelte  # Pokémon Home tab — box management
│   │   ├── notification/+page.svelte # Notifications tab — Discord/LINE
│   │   └── others/+page.svelte   # Other tab — profiles, themes, mouse stick
│   │
│   ├── lib/
│   │   ├── index.ts              # Barrel export
│   │   ├── theme.ts              # Theme system (light/dark/custom)
│   │   ├── assets/
│   │   │   └── favicon.svg       # App favicon
│   │   │
│   │   ├── api/
│   │   │   ├── client.ts         # APIClient class + typed methods
│   │   │   ├── types.ts          # OpenAPI-generated + manual types
│   │   │   ├── websocket.ts      # WebSocket client (event-based, reconnection)
│   │   │   └── datachannel.ts    # WebRTC DataChannel client (WS fallback)
│   │   │
│   │   └── components/
│   │       ├── MenuBar.svelte        # Desktop-style menu bar
│   │       ├── NavBar.svelte         # Tab navigation bar
│   │       ├── StatusBar.svelte      # Connection status footer
│   │       ├── ThemeProvider.svelte  # Theme initialization on mount
│   │       ├── SoftwareController.svelte  # Virtual gamepad
│   │       ├── CaptureRegion.svelte  # Region-of-interest selector (canvas)
│   │       ├── LogPanel.svelte       # Filterable log viewer
│   │       └── OutputPanel.svelte    # Tabbed output panel (text/image/table)
│   │
│   └── api/
│       └── client.js             # Legacy JS client (keep during migration)
```

### Key Files Explained

| File | Purpose |
|------|---------|
| `svelte.config.js` | SPA adapter, `/ui` base path, runes mode enforcement |
| `vite.config.ts` | Dev proxy: `/api` → `localhost:8020`, `/ws` → `ws://localhost:8020` |
| `src/app.html` | HTML entry with PWA meta tags, manifest link |
| `src/service-worker.ts` | Cache-first for assets, network-first for navigation |
| `src/routes/+layout.svelte` | Root layout composing MenuBar, NavBar, StatusBar |
| `src/lib/api/client.ts` | Typed REST API client with all endpoint methods |
| `src/lib/api/websocket.ts` | Reconnecting WebSocket with typed message events |
| `src/lib/api/datachannel.ts` | WebRTC DataChannel with WebSocket fallback |
| `src/lib/theme.ts` | Theme state management (light/dark/custom) |

---

## 3. Development Workflow

### Quick Start (within `nix develop`)

```bash
# Start the Tauri dev server (Rust backend + web frontend)
nix run .#tauri-dev

# Or run only the web frontend (requires Rust backend on :8020)
cd web && npm run dev
```

The Vite dev server starts on `http://localhost:5173` with hot module replacement.
API requests to `/api/*` are proxied to `http://127.0.0.1:8020`.

### Available Scripts

| Command | Description |
|---------|-------------|
| `npm run dev` | Start Vite dev server (HMR) |
| `npm run build` | Production build to `dist/` |
| `npm run preview` | Preview production build |
| `npm run check` | TypeScript + Svelte check |
| `npm run test` | Run Vitest tests |
| `npm run lint` | ESLint |
| `npm run generate-api-types` | Regenerate `src/lib/api/types.ts` from OpenAPI |
| `nix run .#web-check` | CI check: eslint + svelte-check + vitest |

### Development Proxy

When running `npm run dev`, the Vite config proxies:

- `/api/*` → `http://127.0.0.1:8020` (REST API)
- `/ws` → `ws://127.0.0.1:8020` (WebSocket)

This means you can develop the frontend independently while the Rust backend
runs on port 8020.

### Generated API Types

```bash
# After launching the backend, regenerate types
npm run generate-api-types
```

This runs `openapi-typescript` against `http://localhost:8020/api/openapi.json`
and writes to `src/lib/api/types.ts`.

---

## 4. Build Instructions

### Production Build

```bash
# Via nix (recommended)
nix run .#tauri-build

# Manual
cd web
npm ci
npm run build
```

The production build outputs to `web/dist/` (configured in `svelte.config.js`).
When building via `tauri-build`, the Rust backend binary and the web static
assets are bundled together into a Tauri v2 desktop application.

### CI Check

```bash
nix run .#web-check
```

This runs ESLint, Svelte check, and Vitest in sequence.  The same check runs
in CI as the `web-check` job in `.github/workflows/lint.yml`.

### Manual nix Build Targets

| Target | Command | Description |
|--------|---------|-------------|
| Tauri dev | `nix run .#tauri-dev` | Dev server with hot reload |
| Tauri build | `nix run .#tauri-build` | Production bundle |
| Web check | `nix run .#web-check` | TypeScript + lint + test |
| Full check | `nix run .#check` | Rust + Python + web checks |

---

## 5. API Client Usage

### REST API Client (`APIClient`)

The `APIClient` class in `src/lib/api/client.ts` provides typed methods for every
backend endpoint.  It uses the `fetch` API with automatic JSON serialisation.

```typescript
import { api, type StatusResponse, type SerialPort } from '$lib/api/client';

// ── Status ────────────────────────────────────────────────────────────
const status: StatusResponse = await api.getStatus();
// → { camera: boolean, serial: boolean, ws_connected: boolean }

// ── Serial ────────────────────────────────────────────────────────────
const ports: SerialPort[] = await api.getSerialPorts();
await api.openSerial({ port_num: 3, port_name: '/dev/ttyUSB0', baudrate: 115200 });
await api.writeSerial('some data');

// ── Camera ────────────────────────────────────────────────────────────
const devices = await api.getCameras();
await api.openCamera({ device_index: 0, width: 640, height: 480 });
const frame = await api.getCameraFrame();  // → { frame: "base64..." }

// ── Input ─────────────────────────────────────────────────────────────
await api.sendInput('press', { buttons: ['A'], duration: 0.1 });
await api.sendInput('stick', { stick: 'left', direction: 'up' });

// ── Commands ──────────────────────────────────────────────────────────
const commands = await api.getCommands();
await api.startCommand('MyScript');
await api.stopCommand();
```

### Typed WebSocket Client

The `WebSocketClient` provides an event-based interface with automatic
reconnection (exponential backoff, up to 10 retries).

```typescript
import { wsClient } from '$lib/api/client';
import type { WSMessage, LogMessage, FrameMessage } from '$lib/api/client';

wsClient.on('connect', () => console.log('Connected'));
wsClient.on('disconnect', () => console.log('Disconnected'));

wsClient.on('message', (msg: WSMessage) => {
  switch (msg.type) {
    case 'log':
      console.log(`[${msg.level}] ${msg.message}`);
      break;
    case 'frame':
      updateCameraImage(msg.data);  // base64 JPEG
      break;
    case 'status':
      updateConnectionStatus(msg);
      break;
  }
});

wsClient.connect();

// Send typed JSON
wsClient.send({ type: 'echo', payload: 'hello' });
```

### OpenAPI-Generated Types

The `types.ts` file contains auto-generated TypeScript interfaces from the
backend's OpenAPI spec.  The `APIClient` re-exports request schemas:

```typescript
import type {
  CameraConfigRequest,
  PressRequest,
  StickRequest,
  OpenRequest,
  SuccessResponse,
  StatusResponse,
} from '$lib/api/client';
```

Response types that utoipa does not emit (e.g. `StatusResponse`, `SerialPort`,
`CameraDevice`) are manually defined alongside the generated types.

---

## 6. Component Guidelines

### Layout Structure

The UI follows a **Tkinter-inspired layout** with:

```
┌──────────────────────────────────────────────────────┐
│  MenuBar (File / Edit / View / Help)                  │
├──────────────────────────────────────────────────────┤
│  NavBar (Camera / Serial / Manual / Commands / ...)   │
├──────────────────────────────────────────────────────┤
│                                                        │
│  Main Content (tab page)                                │
│                                                        │
├──────────────────────────────────────────────────────┤
│  StatusBar (WebSocket ● / Serial ● / Camera ●)        │
└──────────────────────────────────────────────────────┘
```

### 6 Tabs + Right Panel Pattern

The main navigation offers the following tabs (matching the original Tkinter UI):

| # | Tab | Route | Description |
|---|-----|-------|-------------|
| 1 | Camera | `/ui/camera` | Camera preview, device config, capture |
| 2 | Serial | `/ui/serial` | Port selection, connect/disconnect, send |
| 3 | Manual | `/ui/manual` | Virtual gamepad, mouse stick control |
| 4 | Commands | `/ui/commands` | Script list, filter, start/stop |
| 5 | Key Config | `/ui/keyconfig` | Keyboard-to-button mapping |
| 6 | Pokémon Home | `/ui/pokemonhome` | Box data import/export |
| 7 | Notifications | `/ui/notification` | Discord/LINE/webhook config |
| 8 | Others | `/ui/others` | Profiles, themes, mouse stick settings |

Each tab page follows a consistent pattern:
1. **Title** (`<h2>`)
2. **Content grid** (responsive 1–2 columns)
3. **Output panel** (text/image/table widget selector)
4. **Log panel** (filterable, auto-scrolling)

### Component Patterns

**Use `$state()` for local reactive state:**

```svelte
<script lang="ts">
  let count = $state(0);
  let items = $state<string[]>([]);
  let config = $state<{ enabled: boolean }>({ enabled: false });
</script>
```

**Use `$derived()` for computed values:**

```svelte
<script lang="ts">
  let items = $state([...]);
  let filter = $state('');
  let filtered = $derived(
    filter ? items.filter(i => i.name.includes(filter)) : items
  );
</script>
```

**Use `$effect()` for side effects (auto-scroll, DOM updates):**

```svelte
<script lang="ts">
  let container: HTMLDivElement;
  let autoScroll = $state(true);

  $effect(() => {
    if (autoScroll && container) {
      container.scrollTop = container.scrollHeight;
    }
  });
</script>
```

**Use `$props()` for component inputs:**

```svelte
<script lang="ts">
  let { title = 'Panel', items = [] }: {
    title?: string;
    items?: string[];
  } = $props();
</script>
```

### Writing New Components

1. Create file in `src/lib/components/`
2. Use `lang="ts"` in script tag
3. Use Tailwind utility classes for styling
4. Expose event customisation via `$props()`
5. For shared state, use the event-based patterns in `websocket.ts` or
   pass callbacks via props

### Tailwind + Theme System

The theme system defines CSS custom properties on `:root`:

```css
--color-bg-primary: #0f172a;     /* main background */
--color-bg-card: #1e293b;        /* card/panel background */
--color-text-primary: #f1f5f9;   /* primary text */
--color-accent: #60a5fa;         /* accent/highlight */
```

Components should prefer these variables for theme-aware styling:

```svelte
<div
  class="rounded border p-3"
  style="background-color: var(--color-bg-card); color: var(--color-text-primary);"
>
```

For simple cases, Tailwind classes like `bg-gray-900 text-gray-200` work
well and match the default dark theme.

---

## 7. Communication Protocols

### Transport Layers

The UI supports a layered communication model with automatic fallback:

```
┌──────────────────────────────────────────────┐
│  WebRTC DataChannel  (low-latency, P2P)      │
│       ↓ (fallback on failure)                │
│  WebSocket  (persistent, reliable)            │
│       ↓ (fallback for polling)               │
│  HTTP Fetch  (request-response)               │
└──────────────────────────────────────────────┘
```

### WebSocket (`websocket.ts`)

- **Endpoint**: `ws[s]://<host>/ws`
- **Messages**: JSON with tagged `type` field
- **Auto-reconnect**: Exponential backoff (1s → 30s), up to 10 retries
- **Event system**: `on('connect')`, `on('disconnect')`, `on('message')`

**Message types:**

| Type | Payload | Direction |
|------|---------|-----------|
| `log` | `{ timestamp, level, message }` | Server → Client |
| `status` | `{ camera, serial, ws_connected }` | Server → Client |
| `frame` | `{ data: "base64..." }` | Server → Client |
| `command` | `{ name, running }` | Server → Client |
| `serial` | `{ connected, port_name?, baudrate? }` | Server → Client |
| `camera` | `{ opened, device_index? }` | Server → Client |

### WebRTC DataChannel (`datachannel.ts`)

- **Purpose**: Low-latency bidirectional binary/text communication
- **Signalling**: Uses the WebSocket connection for SDP negotiation
- **Fallback**: If WebRTC is unavailable or the connection fails, falls
  back to the secondary WebSocket client
- **ICE Servers**: Uses Google's public STUN server by default

```typescript
import { DataChannelClient } from '$lib/api/datachannel';
import { wsClient } from '$lib/api/client';

const dc = new DataChannelClient(
  wsClient,        // signalling channel
  wsClient,        // fallback channel
  { label: 'gamepad', rtcEnabled: true }
);

dc.on('connect', () => console.log('DataChannel connected'));
dc.on('message', (data: string) => handleGamepadInput(data));
dc.connect();
```

### Camera Streaming

Two camera streaming methods are available:

1. **Polling (current)**: `GET /api/camera/frame` returns a base64 JPEG.
   The camera page polls every ~100ms using `setTimeout` recursion.

2. **WebSocket push (planned)**: The server streams frames via the WebSocket
   using `{ type: 'frame', data: "base64..." }` messages.  The client rate-limits
   to ~30fps.

3. **MJPEG stream (future)**: A dedicated `/api/camera/stream.mjpeg` endpoint
   for true streaming via `<img>` tag with `Content-Type: multipart/x-mixed-replace`.

---

## 8. Routing & Navigation

### SvelteKit File-Based Routing

All pages are in `src/routes/`.  The app uses SPA mode (`adapter-static`),
so all routing is client-side:

```
src/routes/
├── +layout.svelte       # Root layout (shared shell)
├── +page.svelte         # /ui/ — home page
├── camera/+page.svelte  # /ui/camera
├── serial/+page.svelte  # /ui/serial
├── manual/+page.svelte  # /ui/manual
├── commands/+page.svelte # /ui/commands
├── keyconfig/+page.svelte # /ui/keyconfig
├── notification/+page.svelte # /ui/notification
├── pokemonhome/+page.svelte # /ui/pokemonhome
└── others/+page.svelte  # /ui/others
```

### Base Path

All routes are served under `/ui/` base path:

```js
// svelte.config.js
kit: {
  paths: { base: '/ui' },
}
```

Use `$app/paths` to generate correct URLs:

```svelte
<script lang="ts">
  import { base } from '$app/paths';
</script>
<a href="{base}/camera">Camera</a>
```

### Navigation Bar

The `NavBar` component (`src/lib/components/NavBar.svelte`) renders the tab
links using `$page.url.pathname` for active-state detection:

```svelte
<a
  href="{base}{href}"
  aria-current={$page.url.pathname === `${base}${href}` ? 'page' : undefined}
>
```

---

## 9. Testing

### Vitest (Frontend Tests)

```bash
npm run test
# or
npx vitest --run
```

Tests go in `src/**/*.{test,spec}.{js,ts}` and use Vitest with Node environment.
The config is in `vitest.config.ts` (extends `vite.config.ts`).

### Related Backend Tests

```bash
# Rust tests
cargo test -p pokecon-serial
cargo test -p pokecon-events

# Python tests
pytest tests/ -v
```

### CI Pipeline

The `web-check` CI job runs:
1. ESLint (`npm run lint`)
2. Svelte Check (`npm run check`)
3. Vitest (`npm run test`)

Invoke locally with:
```bash
nix run .#web-check
```

---

## 10. PWA & Service Worker

### Service Worker (`src/service-worker.ts`)

The service worker provides offline support with a dual-cache strategy:

- **Asset cache** (`ASSET_CACHE`): Cache-first for static assets (JS, CSS, images)
- **Navigation cache** (`NAV_CACHE`): Network-first for page navigations with
  offline fallback

**Cache invalidation**: On `activate`, old caches are pruned using the build
`version` hash.

### PWA Manifest

Linked from `src/app.html`:

```html
<link rel="manifest" href="/ui/manifest.json" />
```

The manifest provides:
- App name & icons (192px, 512px)
- Display mode: `standalone`
- Theme colour: `#1a1a2e`

### Registration

The service worker is registered in the root layout (`+layout.svelte`),
only in production mode:

```typescript
import { dev } from '$app/environment';
import { base } from '$app/paths';

onMount(() => {
  if (!dev && 'serviceWorker' in navigator) {
    navigator.serviceWorker.register(`${base}/service-worker.js`);
  }
});
```

---

## Migration Notes (React → SvelteKit)

| Old (React) | New (SvelteKit) |
|-------------|-----------------|
| `components/Dashboard.jsx` | `routes/camera/+page.svelte` |
| `components/Controller.jsx` | `lib/components/SoftwareController.svelte` |
| `components/Scripts.jsx` | `routes/commands/+page.svelte` |
| `components/Settings.jsx` | `routes/serial/+page.svelte` + route per tab |
| `api/client.js` | `lib/api/client.ts` (typed) |
| `App.jsx` (router) | `routes/+layout.svelte` |
| `styles.css` | Tailwind + CSS custom props |
| `index.html` | `app.html` |

### What Changed

- **React 19 + Vite 6** → **SvelteKit 2 + Svelte 5 (runes)**
- **CSS Modules** → **Tailwind CSS 3**
- **JSX components** → **Svelte SFCs**
- **useState/useEffect** → **$state / $derived / $effect**
- **React Router** → **SvelteKit file-based routing**
- **WebSocket event handlers** → **Typed event-based WebSocketClient**
- **Manual fetch() calls** → **Typed APIClient with OpenAPI codegen**
- **No tests** → **Vitest test suite**

---

## Related Documentation

- [Developer Guide](developer-guide.md) — Architecture, build system, Rust/Python
- [User Guide](user-guide.md) — Installation, basic operations
- [API Reference](api-reference.md) — Full REST/WebSocket API spec
- [Script Guide](script-guide.md) — PythonCommand API, image recognition
- [Architecture Report](../architecture_report.md) — Crate-by-crate analysis
