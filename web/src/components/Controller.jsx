import React, { useState, useCallback } from 'react';

export default function Controller({ onInput, serialOpen }) {
  const [stickLeft, setStickLeft] = useState({ x: 128, y: 128 });
  const [stickRight, setStickRight] = useState({ x: 128, y: 128 });
  const [touchPos, setTouchPos] = useState(null);

  const sendButton = useCallback((buttons, duration = 0.1) => {
    onInput('press', { buttons, duration, wait: 0.1 });
  }, [onInput]);

  const sendStick = useCallback((stick, x, y, duration = 0.1) => {
    onInput('stick', { stick, x, y, duration });
  }, [onInput]);

  const sendTouch = useCallback((x, y, duration = 0.1) => {
    onInput('touch', { x, y, duration });
  }, [onInput]);

  const handleStickMove = (stick, e) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const x = Math.max(0, Math.min(255, Math.round(((e.clientX - rect.left) / rect.width) * 255)));
    const y = Math.max(0, Math.min(255, Math.round(((e.clientY - rect.top) / rect.height) * 255)));
    
    if (stick === 'left') setStickLeft({ x, y });
    else setStickRight({ x, y });
  };

  const handleStickEnd = (stick) => {
    if (stick === 'left') {
      sendStick('LSTICK', 128, 128, 0);
      setStickLeft({ x: 128, y: 128 });
    } else {
      sendStick('RSTICK', 128, 128, 0);
      setStickRight({ x: 128, y: 128 });
    }
  };

  const handleTouch = (e) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const x = Math.round(((e.clientX - rect.left) / rect.width) * 320);
    const y = Math.round(((e.clientY - rect.top) / rect.height) * 240);
    setTouchPos({ x, y });
    sendTouch(x, y, 0.1);
    setTimeout(() => setTouchPos(null), 200);
  };

  if (!serialOpen) {
    return (
      <div className="controller disabled">
        <div className="controller-overlay">
          <p>🔌 シリアルポート未接続</p>
          <p className="text-muted">設定から接続してください</p>
        </div>
      </div>
    );
  }

  return (
    <div className="controller">
      {/* D-Pad */}
      <div className="dpad">
        <button className="dpad-btn up" onClick={() => sendButton('DPAD_UP')}>▲</button>
        <button className="dpad-btn left" onClick={() => sendButton('DPAD_LEFT')}>◀</button>
        <button className="dpad-btn right" onClick={() => sendButton('DPAD_RIGHT')}>▶</button>
        <button className="dpad-btn down" onClick={() => sendButton('DPAD_DOWN')}>▼</button>
      </div>

      {/* ABXY */}
      <div className="face-buttons">
        <button className="face-btn x" onClick={() => sendButton('X')}>X</button>
        <button className="face-btn y" onClick={() => sendButton('Y')}>Y</button>
        <button className="face-btn a" onClick={() => sendButton('A')}>A</button>
        <button className="face-btn b" onClick={() => sendButton('B')}>B</button>
      </div>

      {/* Shoulder buttons */}
      <div className="shoulder-buttons">
        <button className="shoulder-btn" onClick={() => sendButton('L')}>L</button>
        <button className="shoulder-btn" onClick={() => sendButton('R')}>R</button>
        <button className="shoulder-btn z" onClick={() => sendButton('ZL')}>ZL</button>
        <button className="shoulder-btn z" onClick={() => sendButton('ZR')}>ZR</button>
      </div>

      {/* System buttons */}
      <div className="system-buttons">
        <button className="sys-btn" onClick={() => sendButton('MINUS')}>−</button>
        <button className="sys-btn home" onClick={() => sendButton('HOME')}>🏠</button>
        <button className="sys-btn" onClick={() => sendButton('PLUS')}>+</button>
        <button className="sys-btn capture" onClick={() => sendButton('CAPTURE')}>📷</button>
      </div>

      {/* Sticks */}
      <div className="sticks">
        <div
          className="stick-area"
          onPointerMove={(e) => e.buttons && handleStickMove('left', e)}
          onPointerUp={() => handleStickEnd('left')}
          onPointerLeave={() => handleStickEnd('left')}
        >
          <div
            className="stick-knob"
            style={{
              left: `${(stickLeft.x / 255) * 100}%`,
              top: `${(stickLeft.y / 255) * 100}%`,
            }}
          />
          <label>L</label>
        </div>
        <div
          className="stick-area"
          onPointerMove={(e) => e.buttons && handleStickMove('right', e)}
          onPointerUp={() => handleStickEnd('right')}
          onPointerLeave={() => handleStickEnd('right')}
        >
          <div
            className="stick-knob"
            style={{
              left: `${(stickRight.x / 255) * 100}%`,
              top: `${(stickRight.y / 255) * 100}%`,
            }}
          />
          <label>R</label>
        </div>
      </div>

      {/* Touchscreen */}
      <div className="touch-panel">
        <div className="touch-area" onClick={handleTouch}>
          {touchPos && (
            <div
              className="touch-dot"
              style={{
                left: `${(touchPos.x / 320) * 100}%`,
                top: `${(touchPos.y / 240) * 100}%`,
              }}
            />
          )}
          <span className="touch-label">タッチスクリーン</span>
        </div>
      </div>
    </div>
  );
}
