export class APIClient {
  constructor(baseUrl) {
    this.baseUrl = baseUrl;
  }

  async _fetch(path, options = {}) {
    const url = `${this.baseUrl}${path}`;
    const resp = await fetch(url, {
      ...options,
      headers: {
        'Content-Type': 'application/json',
        ...options.headers,
      },
    });
    if (!resp.ok) {
      const err = await resp.json().catch(() => ({ error: `HTTP ${resp.status}` }));
      throw new Error(err.error || `HTTP ${resp.status}`);
    }
    return resp.json();
  }

  // Status
  getStatus() { return this._fetch('/api/status'); }

  // Serial
  getSerialPorts() { return this._fetch('/api/serial/ports'); }
  getSerialStatus() { return this._fetch('/api/serial/status'); }
  openSerial({ port_num, port_name, baudrate }) {
    return this._fetch('/api/serial/open', {
      method: 'POST',
      body: JSON.stringify({ port_num, port_name, baudrate }),
    });
  }
  closeSerial() { return this._fetch('/api/serial/close', { method: 'POST' }); }
  updateSerialConfig(config) {
    return this._fetch('/api/serial/config', {
      method: 'POST',
      body: JSON.stringify(config),
    });
  }
  writeSerial(data) {
    return this._fetch('/api/serial/write', {
      method: 'POST',
      body: JSON.stringify({ data }),
    });
  }

  // Camera
  getCameras() { return this._fetch('/api/cameras'); }
  getCameraStatus() { return this._fetch('/api/camera/status'); }
  openCamera({ device_index, width, height }) {
    return this._fetch('/api/camera/open', {
      method: 'POST',
      body: JSON.stringify({ device_index, width, height }),
    });
  }
  closeCamera() { return this._fetch('/api/camera/close', { method: 'POST' }); }
  getCameraFrame() { return this._fetch('/api/camera/frame'); }
  captureCamera(filename) {
    return this._fetch('/api/camera/capture', {
      method: 'POST',
      body: JSON.stringify({ filename }),
    });
  }

  updateCameraConfig(config) {
    return this._fetch('/api/camera/config', {
      method: 'POST',
      body: JSON.stringify(config),
    });
  }

  // Input
  sendInput(type, params) {
    return this._fetch(`/api/input/${type}`, {
      method: 'POST',
      body: JSON.stringify(params),
    });
  }

  // Controller settings
  getControllerType() { return this._fetch('/api/controller/type'); }
  setControllerType(gamepad_type) {
    return this._fetch('/api/controller/type', {
      method: 'POST',
      body: JSON.stringify({ gamepad_type }),
    });
  }
  getKeyboardEnabled() { return this._fetch('/api/controller/keyboard'); }
  setKeyboardEnabled(enabled) {
    return this._fetch('/api/controller/keyboard', {
      method: 'POST',
      body: JSON.stringify({ enabled }),
    });
  }

  // Commands
  getCommands() { return this._fetch('/api/commands'); }
  loadCommand(name) {
    return this._fetch('/api/commands/load', {
      method: 'POST',
      body: JSON.stringify({ name }),
    });
  }
  startCommand(name) {
    return this._fetch('/api/commands/start', {
      method: 'POST',
      body: JSON.stringify({ name }),
    });
  }
  stopCommand() { return this._fetch('/api/commands/stop', { method: 'POST' }); }
  getActiveCommand() { return this._fetch('/api/commands/active'); }
}

export class WebSocketClient {
  constructor(url) {
    this.url = url;
    this.ws = null;
    this.onOpen = null;
    this.onClose = null;
    this.onMessage = null;
    this.reconnectInterval = 3000;
    this.shouldReconnect = true;
  }

  connect() {
    this.shouldReconnect = true;
    this._connect();
  }

  _connect() {
    try {
      this.ws = new WebSocket(this.url);
      this.ws.onopen = () => {
        if (this.onOpen) this.onOpen();
      };
      this.ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          if (this.onMessage) this.onMessage(data);
        } catch (e) {
          console.warn('WS parse error:', e);
        }
      };
      this.ws.onclose = () => {
        if (this.onClose) this.onClose();
        if (this.shouldReconnect) {
          setTimeout(() => this._connect(), this.reconnectInterval);
        }
      };
      this.ws.onerror = (err) => {
        console.error('WS error:', err);
      };
    } catch (err) {
      console.error('WS connect error:', err);
      if (this.shouldReconnect) {
        setTimeout(() => this._connect(), this.reconnectInterval);
      }
    }
  }

  disconnect() {
    this.shouldReconnect = false;
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  send(data) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(data));
    }
  }
}
