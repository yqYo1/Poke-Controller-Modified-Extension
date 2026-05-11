import React, { useState, useEffect } from 'react';

export default function Settings({ serialOpen, onSerialOpen, onSerialClose, api }) {
  const [ports, setPorts] = useState([]);
  const [selectedPort, setSelectedPort] = useState('');
  const [baudrate, setBaudrate] = useState(9600);
  const [dataFormat, setDataFormat] = useState('Default');
  const [customPort, setCustomPort] = useState('');

  // Camera state
  const [cameras, setCameras] = useState([]);
  const [selectedCamera, setSelectedCamera] = useState('');
  const [camWidth, setCamWidth] = useState(1280);
  const [camHeight, setCamHeight] = useState(720);
  const [camFps, setCamFps] = useState(30);
  const [cameraOpen, setCameraOpen] = useState(false);
  const [cameraInfo, setCameraInfo] = useState(null);

  useEffect(() => {
    loadPorts();
    loadCameras();
  }, []);

  const loadPorts = async () => {
    try {
      const data = await api.getSerialPorts();
      setPorts(data.ports || []);
    } catch (err) {
      console.error('Failed to load ports:', err);
    }
  };

  // ── Camera ──────────────────────────────────────────────

  const loadCameras = async () => {
    try {
      const data = await api.getCameras();
      setCameras(data.devices || []);
    } catch (err) {
      console.error('Failed to load cameras:', err);
    }
  };

  const loadCameraStatus = async () => {
    try {
      const data = await api.getCameraStatus();
      setCameraOpen(data.is_open || false);
      if (data.is_open) {
        setCameraInfo(data);
        setSelectedCamera(String(data.device_index));
        setCamWidth(data.width);
        setCamHeight(data.height);
        setCamFps(data.fps);
      }
    } catch (err) {
      console.error('Failed to get camera status:', err);
    }
  };

  const handleCameraOpen = async () => {
    if (!selectedCamera) return;
    try {
      const data = await api.openCamera({
        device_index: parseInt(selectedCamera, 10),
        width: camWidth,
        height: camHeight,
      });
      if (data.status === 'ok') {
        setCameraOpen(true);
        await loadCameraStatus();
      }
    } catch (err) {
      console.error('Failed to open camera:', err);
    }
  };

  const handleCameraClose = async () => {
    try {
      await api.closeCamera();
      setCameraOpen(false);
      setCameraInfo(null);
    } catch (err) {
      console.error('Failed to close camera:', err);
    }
  };

  useEffect(() => {
    loadCameraStatus();
  }, []);

  const handleConnect = () => {
    const portName = customPort || selectedPort;
    if (!portName) return;
    onSerialOpen({ port_name: portName, baudrate });
  };

  const handleBaudrateChange = async (newBaudrate) => {
    setBaudrate(newBaudrate);
    try {
      await api.updateSerialConfig({ baudrate: newBaudrate });
    } catch (err) {
      console.error('Failed to update baudrate:', err);
    }
  };

  const handleDataFormatChange = async (newFormat) => {
    setDataFormat(newFormat);
    try {
      await api.updateSerialConfig({ data_format: newFormat });
    } catch (err) {
      console.error('Failed to update data format:', err);
    }
  };

  return (
    <div className="settings">
      <section className="panel">
        <div className="panel-header">
          <h2>🔌 シリアル接続</h2>
        </div>

        <div className="form-group">
          <label>ポート</label>
          <select
            value={selectedPort}
            onChange={(e) => setSelectedPort(e.target.value)}
            disabled={serialOpen}
          >
            <option value="">ポートを選択</option>
            {ports.map((port) => (
              <option key={port} value={port}>{port}</option>
            ))}
          </select>
        </div>

        <div className="form-group">
          <label>または直接入力</label>
          <input
            type="text"
            value={customPort}
            onChange={(e) => setCustomPort(e.target.value)}
            placeholder="/dev/ttyUSB0 または COM3"
            disabled={serialOpen}
          />
        </div>

        <div className="form-group">
          <label>ボーレート</label>
          <select
            value={baudrate}
            onChange={(e) => handleBaudrateChange(Number(e.target.value))}
          >
            <option value={9600}>9600</option>
            <option value={19200}>19200</option>
            <option value={38400}>38400</option>
            <option value={57600}>57600</option>
            <option value={115200}>115200</option>
          </select>
        </div>

        <div className="form-group">
          <label>データフォーマット</label>
          <select
            value={dataFormat}
            onChange={(e) => handleDataFormatChange(e.target.value)}
          >
            <option value="Default">Default</option>
            <option value="Qingpi">Qingpi</option>
            <option value="3DS Controller">3DS Controller</option>
          </select>
        </div>

        <div className="button-row">
          {!serialOpen ? (
            <button className="btn btn-primary" onClick={handleConnect}>
              🔌 接続
            </button>
          ) : (
            <button className="btn btn-danger" onClick={onSerialClose}>
              🔌 切断
            </button>
          )}
          <button className="btn btn-secondary" onClick={loadPorts} disabled={serialOpen}>
            🔄 ポート更新
          </button>
        </div>
      </section>

      <section className="panel">
        <div className="panel-header">
          <h2>📷 カメラ設定</h2>
        </div>

        <div className="form-group">
          <label>カメラデバイス</label>
          <select
            value={selectedCamera}
            onChange={(e) => setSelectedCamera(e.target.value)}
            disabled={cameraOpen}
          >
            <option value="">カメラを選択</option>
            {cameras.map((cam) => (
              <option key={cam.index} value={cam.index}>
                {cam.card || cam.name || `/dev/video${cam.index}`}
              </option>
            ))}
          </select>
        </div>

        <div className="form-row">
          <div className="form-group">
            <label>幅 (px)</label>
            <input
              type="number"
              value={camWidth}
              onChange={(e) => setCamWidth(Number(e.target.value))}
              disabled={cameraOpen}
              min={160}
              max={3840}
              step={16}
            />
          </div>
          <div className="form-group">
            <label>高さ (px)</label>
            <input
              type="number"
              value={camHeight}
              onChange={(e) => setCamHeight(Number(e.target.value))}
              disabled={cameraOpen}
              min={120}
              max={2160}
              step={16}
            />
          </div>
        </div>

        <div className="form-group">
          <label>フレームレート (FPS)</label>
          <select
            value={camFps}
            onChange={(e) => setCamFps(Number(e.target.value))}
            disabled={cameraOpen}
          >
            <option value={15}>15</option>
            <option value={30}>30</option>
            <option value={60}>60</option>
          </select>
        </div>

        {cameraInfo && (
          <div className="info-list" style={{ marginBottom: '8px' }}>
            <div className="info-item">
              <span className="info-label">デバイス</span>
              <span className="info-value">{`/dev/video${cameraInfo.device_index}`}</span>
            </div>
            <div className="info-item">
              <span className="info-label">解像度</span>
              <span className="info-value">{`${cameraInfo.width}×${cameraInfo.height}`}</span>
            </div>
            <div className="info-item">
              <span className="info-label">FPS</span>
              <span className="info-value">{cameraInfo.fps}</span>
            </div>
          </div>
        )}

        <div className="button-row">
          {!cameraOpen ? (
            <button className="btn btn-primary" onClick={handleCameraOpen}>
              📷 カメラを開く
            </button>
          ) : (
            <button className="btn btn-danger" onClick={handleCameraClose}>
              📷 カメラを閉じる
            </button>
          )}
          <button className="btn btn-secondary" onClick={loadCameras} disabled={cameraOpen}>
            🔄 デバイス更新
          </button>
        </div>
      </section>

      <section className="panel">
        <div className="panel-header">
          <h2>ℹ️ 情報</h2>
        </div>
        <div className="info-list">
          <div className="info-item">
            <span className="info-label">バージョン</span>
            <span className="info-value">0.1.0</span>
          </div>
          <div className="info-item">
            <span className="info-label">Rust コア</span>
            <span className="info-value">✅ 有効</span>
          </div>
          <div className="info-item">
            <span className="info-label">PyO3 バインディング</span>
            <span className="info-value">✅ 有効</span>
          </div>
        </div>
      </section>
    </div>
  );
}
