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

  useEffect(() => { loadPorts(); loadCameras(); loadControllerSettings(); }, []);

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
