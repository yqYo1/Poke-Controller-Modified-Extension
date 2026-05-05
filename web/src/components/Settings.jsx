import React, { useState, useEffect } from 'react';

export default function Settings({ serialOpen, onSerialOpen, onSerialClose, api }) {
  const [ports, setPorts] = useState([]);
  const [selectedPort, setSelectedPort] = useState('');
  const [baudrate, setBaudrate] = useState(9600);
  const [customPort, setCustomPort] = useState('');

  useEffect(() => {
    loadPorts();
  }, []);

  const loadPorts = async () => {
    try {
      const data = await api.getSerialPorts();
      setPorts(data.ports || []);
    } catch (err) {
      console.error('Failed to load ports:', err);
    }
  };

  const handleConnect = () => {
    const portName = customPort || selectedPort;
    if (!portName) return;
    onSerialOpen({ port_name: portName, baudrate });
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
            onChange={(e) => setBaudrate(Number(e.target.value))}
            disabled={serialOpen}
          >
            <option value={9600}>9600</option>
            <option value={19200}>19200</option>
            <option value={38400}>38400</option>
            <option value={57600}>57600</option>
            <option value={115200}>115200</option>
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
