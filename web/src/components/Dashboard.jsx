import React from 'react';

export default function Dashboard({ cameraFrame, cameraOpen, onCameraOpen, onCameraClose, onCapture, flip, onFlipChange, logs, activeCommand }) {
  return (
    <div className="dashboard">
      {/* カメラプレビュー */}
      <section className="panel camera-panel">
        <div className="panel-header">
          <h2>📷 カメラ</h2>
          <button
            className={`btn ${cameraOpen ? 'btn-danger' : 'btn-primary'}`}
            onClick={() => cameraOpen ? onCameraClose() : onCameraOpen({})}
          >
            {cameraOpen ? '切断' : '接続'}
          </button>
        </div>
        <div className="camera-viewport">
          {cameraFrame ? (
            <img
              src={`data:image/jpeg;base64,${cameraFrame}`}
              alt="Camera"
              className={`camera-image${cameraOpen && flip !== 'none' ? ' flipped' : ''}`}
              data-flip={cameraOpen ? flip : 'none'}
            />
          ) : (
            <div className="camera-placeholder">
              <span className="placeholder-icon">📷</span>
              <p>カメラ未接続</p>
            </div>
          )}
        </div>
        {/* Camera controls toolbar */}
        <div className="camera-toolbar">
          <div className="flip-controls">
            <span className="toolbar-label">反転:</span>
            {['none', 'horizontal', 'vertical', 'both'].map((mode) => (
              <button
                key={mode}
                className={`btn btn-flip ${flip === mode ? 'active' : ''}`}
                onClick={() => onFlipChange(mode)}
                disabled={!cameraOpen}
                title={
                  mode === 'none' ? '反転なし' :
                  mode === 'horizontal' ? '水平反転' :
                  mode === 'vertical' ? '垂直反転' : '両方向反転'
                }
              >
                {mode === 'none' ? '⛔' :
                 mode === 'horizontal' ? '↔' :
                 mode === 'vertical' ? '↕' : '🔄'}
              </button>
            ))}
          </div>
          <button
            className="btn btn-capture"
            onClick={onCapture}
            disabled={!cameraOpen}
          >
            📸 キャプチャ
          </button>
        </div>
      </section>

      {/* 実行中コマンド */}
      <section className="panel">
        <div className="panel-header">
          <h2>▶️ 実行中</h2>
        </div>
        <div className="active-command">
          {activeCommand ? (
            <div className="command-running">
              <span className="pulse-dot"></span>
              <span>{activeCommand}</span>
            </div>
          ) : (
            <p className="text-muted">コマンド未実行</p>
          )}
        </div>
      </section>

      {/* ログ */}
      <section className="panel log-panel">
        <div className="panel-header">
          <h2>📝 ログ</h2>
        </div>
        <div className="log-container">
          {logs.map((log, i) => (
            <div key={i} className={`log-line log-${log.level}`}>
              <span className="log-time">{log.time}</span>
              <span className="log-message">{log.message}</span>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
