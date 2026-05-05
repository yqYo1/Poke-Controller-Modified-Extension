/**
 * Poke-Controller Web UI — main entry point.
 *
 * Works identically in both Tauri and web modes.
 * The backend HTTP server runs in both modes, so we always use fetch().
 */

const API_BASE = 'http://127.0.0.1:8020';

/** Log a message to the log panel. */
function log(message, level = 'info') {
  const el = document.getElementById('log-output');
  if (!el) return;
  const line = document.createElement('span');
  line.className = `log-${level}`;
  line.textContent = `[${level.toUpperCase()}] ${message}`;
  el.appendChild(line);
  el.appendChild(document.createElement('br'));
  el.scrollTop = el.scrollHeight;
}

/** Update connection status indicator. */
function setStatus(connected) {
  const el = document.getElementById('status-indicator');
  if (!el) return;
  el.className = `status ${connected ? 'connected' : 'disconnected'}`;
  el.textContent = connected ? '● Connected' : '○ Disconnected';
}

/** Send a greet command — demonstrates API call. */
async function greet(name) {
  try {
    const resp = await fetch(`${API_BASE}/api/greet?name=${encodeURIComponent(name)}`);
    const data = await resp.json();
    log(`Greet: ${data.message}`, 'info');
  } catch (err) {
    log(`Greet failed: ${err}`, 'error');
    setStatus(false);
  }
}

/** Check backend health. */
async function checkHealth() {
  try {
    const resp = await fetch(`${API_BASE}/api/status`);
    if (resp.ok) {
      const data = await resp.json();
      setStatus(true);
      log(`Backend status: ${data.status} (v${data.version})`, 'info');
    } else {
      throw new Error(`HTTP ${resp.status}`);
    }
  } catch {
    setStatus(false);
    log('Backend unreachable — is the server running?', 'warn');
  }
}

// ─── Initialize ──────────────────────────────────────────────────────────

document.addEventListener('DOMContentLoaded', () => {
  log('Poke-Controller UI initializing...', 'info');

  // Greet button handler
  const btn = document.getElementById('btn-greet');
  if (btn) {
    btn.addEventListener('click', () => greet('Poke-Trainer'));
  }

  // Check backend health on startup
  checkHealth();

  // Periodic health check (every 10s)
  setInterval(checkHealth, 10_000);
});
