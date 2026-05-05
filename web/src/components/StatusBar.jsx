import React from 'react';

export default function StatusBar({ connected, serialOpen, cameraOpen }) {
  return (
    <header className="status-bar">
      <div className="status-title">
        <span className="status-logo">🔴</span>
        <h1>Poke-Controller</h1>
      </div>
      <div className="status-indicators">
        <span className={`status-badge ${connected ? 'on' : 'off'}`}>
          {connected ? '🟢' : '🔴'} 接続
        </span>
        <span className={`status-badge ${serialOpen ? 'on' : 'off'}`}>
          {serialOpen ? '🟢' : '⚪'} シリアル
        </span>
        <span className={`status-badge ${cameraOpen ? 'on' : 'off'}`}>
          {cameraOpen ? '🟢' : '⚪'} カメラ
        </span>
      </div>
    </header>
  );
}
