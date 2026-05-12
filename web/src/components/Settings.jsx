import React, { useState, useEffect } from 'react';

export default function Settings({ serialOpen, onSerialOpen, onSerialClose, api, onControllerWindowOpen }) {
  const [ports, setPorts] = useState([]);
  const [selectedPort, setSelectedPort] = useState('');
  const [baudrate, setBaudrate] = useState(9600);
  const [dataFormat, setDataFormat] = useState('Default');
  const [customPort, setCustomPort] = useState('');
  const [cameras, setCameras] = useState([]);
  const [selectedCamera, setSelectedCamera] = useState('');
  const [camWidth, setCamWidth] = useState(1280);
  const [camHeight, setCamHeight] = useState(720);
  const [camFps, setCamFps] = useState(30);
  const [cameraOpen, setCameraOpen] = useState(false);
  const [cameraInfo, setCameraInfo] = useState(null);
  const [gamepadType, setGamepadType] = useState('ProController');
  const [keyboardEnabled, setKeyboardEnabled] = useState(false);
  const [notifyWindows, setNotifyWindows] = useState(true);
  const [notifyLine, setNotifyLine] = useState(false);
  const [notifyDiscord, setNotifyDiscord] = useState(false);
  const [discordWebhookUrl, setDiscordWebhookUrl] = useState("");
  const [lineAccessToken, setLineAccessToken] = useState("");
  const [notifyTestResult, setNotifyTestResult] = useState(null);
  const [theme, setTheme] = useState(() => {
    try { return localStorage.getItem("pokecon-theme") || "dark"; } catch { return "dark"; }
  });
  const [windowWidth, setWindowWidth] = useState(() => {
    try { const v = parseInt(localStorage.getItem("pokecon-win-width") || "1280", 10); return Number.isFinite(v) ? v : 1280; } catch { return 1280; }
  });
  const [windowHeight, setWindowHeight] = useState(() => {
    try { const v = parseInt(localStorage.getItem("pokecon-win-height") || "800", 10); return Number.isFinite(v) ? v : 800; } catch { return 800; }
  });
  const [btnPosition, setBtnPosition] = useState(() => {
    try { return localStorage.getItem("pokecon-btn-position") || "bottom"; } catch { return "bottom"; }
  });

  useEffect(() => { loadPorts(); loadCameras(); loadControllerSettings(); loadNotificationConfig(); }, []);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    try { localStorage.setItem("pokecon-theme", theme); } catch {}
  }, [theme]);

  useEffect(() => {
    try {
      localStorage.setItem("pokecon-win-width", String(windowWidth));
      localStorage.setItem("pokecon-win-height", String(windowHeight));
    } catch {}
  }, [windowWidth, windowHeight]);

  useEffect(() => {
    try { localStorage.setItem("pokecon-btn-position", btnPosition); } catch {}
  }, [btnPosition]);

  const loadPorts = async () => {
    try { const d = await api.getSerialPorts(); setPorts(d.ports || []); }
    catch (e) { console.error(e); }
  };

  const loadControllerSettings = async () => {
    try {
      const td = await api.getControllerType();
      if (td.gamepad_type) setGamepadType(td.gamepad_type);
      const kd = await api.getKeyboardEnabled();
      if (kd.keyboard_enabled !== undefined) setKeyboardEnabled(kd.keyboard_enabled);
    } catch (e) { console.error(e); }
  };

  const handleGamepadTypeChange = async (v) => {
    setGamepadType(v);
    try { await api.setControllerType(v); } catch (e) { console.error(e); }
  };

  const handleKeyboardToggle = async () => {
    const nv = !keyboardEnabled;
    setKeyboardEnabled(nv);
    try { await api.setKeyboardEnabled(nv); } catch (e) { console.error(e); setKeyboardEnabled(!nv); }
  };

  const loadNotificationConfig = async () => {
    try {
      const d = await api.getNotificationConfig();
      if (d.windows_enabled !== undefined) setNotifyWindows(d.windows_enabled);
      if (d.line_enabled !== undefined) setNotifyLine(d.line_enabled);
      if (d.discord_enabled !== undefined) setNotifyDiscord(d.discord_enabled);
      if (d.discord_webhook_url !== undefined) setDiscordWebhookUrl(d.discord_webhook_url);
      if (d.line_access_token !== undefined) setLineAccessToken(d.line_access_token);
    } catch (e) { console.error(e); }
  };

  const handleNotifyConfigChange = async (update) => {
    if (update.windows_enabled !== undefined) setNotifyWindows(update.windows_enabled);
    if (update.line_enabled !== undefined) setNotifyLine(update.line_enabled);
    if (update.discord_enabled !== undefined) setNotifyDiscord(update.discord_enabled);
    if (update.discord_webhook_url !== undefined) setDiscordWebhookUrl(update.discord_webhook_url);
    if (update.line_access_token !== undefined) setLineAccessToken(update.line_access_token);
    try { await api.updateNotificationConfig(update); } catch (e) { console.error(e); }
  };

  const handleSendTestNotification = async () => {
    setNotifyTestResult(null);
    try {
      const d = await api.sendTestNotification({
        message: "This is a test notification from Poke-Controller!",
        title: "Test Notification",
      });
      setNotifyTestResult(d);
    } catch (e) {
      setNotifyTestResult({ status: "error", message: e.message });
    }
  };

  const handleThemeChange = (newTheme) => {
    setTheme(newTheme);
  };

  const handleWindowSizePreset = (preset) => {
    switch (preset) {
      case "small": setWindowWidth(800); setWindowHeight(600); break;
      case "medium": setWindowWidth(1024); setWindowHeight(768); break;
      case "large": setWindowWidth(1280); setWindowHeight(900); break;
      case "xlarge": setWindowWidth(1920); setWindowHeight(1080); break;
      default: break;
    }
  };

  const handleBtnPositionChange = (pos) => {
    setBtnPosition(pos);
  };

  const loadCameras = async () => {
    try { const d = await api.getCameras(); setCameras(d.devices || []); }
    catch (e) { console.error(e); }
  };

  const loadCameraStatus = async () => {
    try {
      const d = await api.getCameraStatus();
      setCameraOpen(d.is_open || false);
      if (d.is_open) { setCameraInfo(d); setSelectedCamera(String(d.device_index)); setCamWidth(d.width); setCamHeight(d.height); setCamFps(d.fps); }
    } catch (e) { console.error(e); }
  };

  const handleCameraOpen = async () => {
    if (!selectedCamera) return;
    try {
      const d = await api.openCamera({ device_index: parseInt(selectedCamera,10), width: camWidth, height: camHeight });
      if (d.status === 'ok') { setCameraOpen(true); await loadCameraStatus(); }
    } catch (e) { console.error(e); }
  };

  const handleCameraClose = async () => { try { await api.closeCamera(); setCameraOpen(false); setCameraInfo(null); } catch(e) { console.error(e); } };

  useEffect(() => { loadCameraStatus(); }, []);

  const handleConnect = () => {
    const portName = customPort || selectedPort;
    if (!portName) return;
    onSerialOpen({ port_name: portName, baudrate });
  };

  const handleBaudrateChange = async (v) => {
    setBaudrate(v);
    try { await api.updateSerialConfig({ baudrate: v }); } catch(e) { console.error(e); }
  };

  const handleDataFormatChange = async (v) => {
    setDataFormat(v);
    try { await api.updateSerialConfig({ data_format: v }); } catch(e) { console.error(e); }
  };

  return (
    <div className="settings">
      <section className="panel">
        <div className="panel-header"><h2>Serial Connection</h2></div>
        <div className="form-group">
          <label>Port</label>
          <select value={selectedPort} onChange={e => setSelectedPort(e.target.value)} disabled={serialOpen}>
            <option value="">Select port</option>
            {ports.map(p => <option key={p} value={p}>{p}</option>)}
          </select>
        </div>
        <div className="form-group">
          <label>Or enter manually</label>
          <input type="text" value={customPort} onChange={e => setCustomPort(e.target.value)} placeholder="/dev/ttyUSB0 or COM3" disabled={serialOpen} />
        </div>
        <div className="form-group">
          <label>Baudrate</label>
          <select value={baudrate} onChange={e => handleBaudrateChange(Number(e.target.value))}>
            <option value={9600}>9600</option><option value={19200}>19200</option><option value={38400}>38400</option><option value={57600}>57600</option><option value={115200}>115200</option>
          </select>
        </div>
        <div className="form-group">
          <label>Data Format</label>
          <select value={dataFormat} onChange={e => handleDataFormatChange(e.target.value)}>
            <option value="Default">Default</option><option value="Qingpi">Qingpi</option><option value="3DS Controller">3DS Controller</option>
          </select>
        </div>
        <div className="button-row">
          {!serialOpen ? <button className="btn btn-primary" onClick={handleConnect}>Connect</button> : <button className="btn btn-danger" onClick={onSerialClose}>Disconnect</button>}
          <button className="btn btn-secondary" onClick={loadPorts} disabled={serialOpen}>Refresh Ports</button>
        </div>
      </section>

      <section className="panel">
        <div className="panel-header"><h2>Controller Settings</h2></div>
        <div className="form-group">
          <label>Gamepad Type</label>
          <select value={gamepadType} onChange={e => handleGamepadTypeChange(e.target.value)}>
            <option value="ProController">Pro Controller</option>
            <option value="Xinput">Xinput</option>
          </select>
        </div>
        <div className="form-group">
          <label><input type="checkbox" checked={keyboardEnabled} onChange={handleKeyboardToggle} style={{marginRight:'8px'}} />Enable Keyboard Input</label>
        </div>
        <div className="button-row">
          <button className="btn btn-primary" onClick={onControllerWindowOpen}>Open Controller Window</button>
        </div>
      </section>

      <section className="panel">
        <div className="panel-header"><h2>Camera Settings</h2></div>
        <div className="form-group">
          <label>Camera Device</label>
          <select value={selectedCamera} onChange={e => setSelectedCamera(e.target.value)} disabled={cameraOpen}>
            <option value="">Select camera</option>
            {cameras.map(c => <option key={c.index} value={c.index}>{c.card || c.name || `/dev/video${c.index}`}</option>)}
          </select>
        </div>
        <div className="form-row">
          <div className="form-group"><label>Width (px)</label><input type="number" value={camWidth} onChange={e => setCamWidth(Number(e.target.value))} disabled={cameraOpen} min={160} max={3840} step={16} /></div>
          <div className="form-group"><label>Height (px)</label><input type="number" value={camHeight} onChange={e => setCamHeight(Number(e.target.value))} disabled={cameraOpen} min={120} max={2160} step={16} /></div>
        </div>
        <div className="form-group">
          <label>Frame Rate (FPS)</label>
          <select value={camFps} onChange={e => setCamFps(Number(e.target.value))} disabled={cameraOpen}>
            <option value={15}>15</option><option value={30}>30</option><option value={60}>60</option>
          </select>
        </div>
        {cameraInfo && <div className="info-list" style={{marginBottom:'8px'}}>
          <div className="info-item"><span className="info-label">Device</span><span className="info-value">{`/dev/video${cameraInfo.device_index}`}</span></div>
          <div className="info-item"><span className="info-label">Resolution</span><span className="info-value">{`${cameraInfo.width}x${cameraInfo.height}`}</span></div>
          <div className="info-item"><span className="info-label">FPS</span><span className="info-value">{cameraInfo.fps}</span></div>
        </div>}
        <div className="button-row">
          {!cameraOpen ? <button className="btn btn-primary" onClick={handleCameraOpen}>Open Camera</button> : <button className="btn btn-danger" onClick={handleCameraClose}>Close Camera</button>}
          <button className="btn btn-secondary" onClick={loadCameras} disabled={cameraOpen}>Refresh Devices</button>
        </div>
      </section>

      <section className="panel">
        <div className="panel-header"><h2>Notification Settings</h2></div>
        <div className="form-group">
          <label><input type="checkbox" checked={notifyWindows} onChange={e => handleNotifyConfigChange({ windows_enabled: e.target.checked })} style={{marginRight:"8px"}} />Windows Desktop Notification</label>
        </div>
        <div className="form-group">
          <label><input type="checkbox" checked={notifyLine} onChange={e => handleNotifyConfigChange({ line_enabled: e.target.checked })} style={{marginRight:"8px"}} />LINE Notification</label>
        </div>
        {notifyLine && <div className="form-group">
          <label>LINE Access Token</label>
          <input type="password" value={lineAccessToken} onChange={e => setLineAccessToken(e.target.value)} onBlur={e => handleNotifyConfigChange({ line_access_token: e.target.value })} placeholder="Enter LINE channel access token" />
        </div>}
        <div className="form-group">
          <label><input type="checkbox" checked={notifyDiscord} onChange={e => handleNotifyConfigChange({ discord_enabled: e.target.checked })} style={{marginRight:"8px"}} />Discord Notification</label>
        </div>
        {notifyDiscord && <div className="form-group">
          <label>Discord Webhook URL</label>
          <input type="password" value={discordWebhookUrl} onChange={e => setDiscordWebhookUrl(e.target.value)} onBlur={e => handleNotifyConfigChange({ discord_webhook_url: e.target.value })} placeholder="https://discord.com/api/webhooks/..." />
        </div>}
        <div className="button-row">
          <button className="btn btn-primary" onClick={handleSendTestNotification}>Send Test Notification</button>
        </div>
        {notifyTestResult && <div className="info-list" style={{marginTop:"8px"}}>
          {Array.isArray(notifyTestResult.results) && notifyTestResult.results.map((r, i) => (
            <div key={i} className="info-item">
              <span className="info-label">{r.channel}</span>
              <span className={"info-value " + (r.status === "sent" ? "text-success" : "text-error")}>{r.status === "sent" ? "\u2713 Sent" : "\u2717 " + (r.error || "Failed")}</span>
            </div>
          ))}
          {notifyTestResult.status === "error" && <div className="info-item">
            <span className="info-label">Error</span>
            <span className="info-value text-error">{notifyTestResult.message}</span>
          </div>}
        </div>}
      </section>

            <section className="panel">
        <div className="panel-header"><h2>Theme and Display</h2></div>
        <div className="form-group">
          <label>Theme</label>
          <div className="theme-toggle">
            <button className={"btn " + (theme === "dark" ? "btn-primary" : "btn-secondary")} onClick={() => handleThemeChange("dark")}>Dark</button>
            <button className={"btn " + (theme === "light" ? "btn-primary" : "btn-secondary")} onClick={() => handleThemeChange("light")}>Light</button>
          </div>
        </div>
        <div className="form-group">
          <label>Window Size</label>
          <div className="theme-toggle">
            <button className="btn btn-secondary" onClick={() => handleWindowSizePreset("small")}>Small</button>
            <button className="btn btn-secondary" onClick={() => handleWindowSizePreset("medium")}>Medium</button>
            <button className="btn btn-secondary" onClick={() => handleWindowSizePreset("large")}>Large</button>
            <button className="btn btn-secondary" onClick={() => handleWindowSizePreset("xlarge")}>X-Large</button>
          </div>
        </div>
        <div className="form-row">
          <div className="form-group"><label>Width (px)</label><input type="number" value={windowWidth} onChange={e => setWindowWidth(Number(e.target.value))} min={640} max={3840} /></div>
          <div className="form-group"><label>Height (px)</label><input type="number" value={windowHeight} onChange={e => setWindowHeight(Number(e.target.value))} min={480} max={2160} /></div>
        </div>
      </section>

            <section className="panel">
        <div className="panel-header"><h2>Button Position</h2></div>
        <div className="form-group">
          <label>Navigation Bar Position</label>
          <div className="theme-toggle">
            <button className={"btn " + (btnPosition === "bottom" ? "btn-primary" : "btn-secondary")} onClick={() => handleBtnPositionChange("bottom")}>Bottom</button>
            <button className={"btn " + (btnPosition === "top" ? "btn-primary" : "btn-secondary")} onClick={() => handleBtnPositionChange("top")}>Top</button>
            <button className={"btn " + (btnPosition === "left" ? "btn-primary" : "btn-secondary")} onClick={() => handleBtnPositionChange("left")}>Left</button>
            <button className={"btn " + (btnPosition === "right" ? "btn-primary" : "btn-secondary")} onClick={() => handleBtnPositionChange("right")}>Right</button>
          </div>
        </div>
      </section>

            <section className="panel">
        <div className="panel-header"><h2>Info</h2></div>
        <div className="info-list">
          <div className="info-item"><span className="info-label">Version</span><span className="info-value">0.1.0</span></div>
          <div className="info-item"><span className="info-label">Rust Core</span><span className="info-value">Enabled</span></div>
          <div className="info-item"><span className="info-label">PyO3 Bindings</span><span className="info-value">Enabled</span></div>
        </div>
      </section>
    </div>
  );
}
