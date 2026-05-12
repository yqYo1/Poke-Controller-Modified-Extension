import React, { useState, useEffect, useCallback, useRef } from 'react';
import Controller from './Controller.jsx';

const STICK_THROTTLE_MS = 16; // ~60fps

export default function ControllerWindow({ visible, onClose, onInput, serialOpen, api }) {
  const [leftEnabled, setLeftEnabled] = useState(false);
  const [rightEnabled, setRightEnabled] = useState(false);
  const [sensitivity, setSensitivity] = useState(1.0);
  const activeStick = useRef(null); // 'left' | 'right' | null
  const mouseDownPos = useRef({ x: 0, y: 0 });
  const lastStickSend = useRef(0);

  // Load initial state when the window opens
  useEffect(() => {
    if (visible && api) {
      api.getMouseStick().then(cfg => {
        setLeftEnabled(cfg.left_enabled);
        setRightEnabled(cfg.right_enabled);
        setSensitivity(cfg.sensitivity);
      }).catch(() => {});
    }
  }, [visible, api]);

  // Enable/disable mouse stick for a specific stick
  const toggleMouseStick = useCallback(async (stick, enable) => {
    if (!api) return;
    try {
      await api.setMouseStick({ stick, enabled: enable, sensitivity });
      if (stick === 'left') setLeftEnabled(enable);
      else setRightEnabled(enable);
    } catch (err) {
      console.error('Failed to set mouse stick:', err);
    }
  }, [api, sensitivity]);

  // Update sensitivity on server
  const handleSensitivityChange = useCallback(async (val) => {
    const v = parseFloat(val);
    setSensitivity(v);
    if (!api) return;
    // Update both left and right with new sensitivity if either is active
    try {
      if (leftEnabled) {
        await api.setMouseStick({ stick: 'left', enabled: true, sensitivity: v });
      }
      if (rightEnabled) {
        await api.setMouseStick({ stick: 'right', enabled: true, sensitivity: v });
      }
    } catch (err) {
      console.error('Failed to update sensitivity:', err);
    }
  }, [api, leftEnabled, rightEnabled]);

  // ── Mouse event handlers ─────────────────────────────────────────────────
  // These are bound/unbound to document when the window is visible
  // and the respective stick mode is enabled.

  const handleMouseDown = useCallback((e) => {
    // Left mouse button → left stick (if enabled)
    if (e.button === 0 && leftEnabled) {
      activeStick.current = 'left';
      mouseDownPos.current = { x: e.clientX, y: e.clientY };
      // Send initial center position
      onInput('stick', { stick: 'left', x: 128, y: 128, duration: 0 });
      e.preventDefault();
    }
    // Right mouse button → right stick (if enabled)
    if (e.button === 2 && rightEnabled) {
      activeStick.current = 'right';
      mouseDownPos.current = { x: e.clientX, y: e.clientY };
      onInput('stick', { stick: 'right', x: 128, y: 128, duration: 0 });
      e.preventDefault();
    }
  }, [leftEnabled, rightEnabled, onInput]);

  const handleMouseMoveRaw = useCallback((e) => {
    const stick = activeStick.current;
    if (!stick) return;

    const dx = e.clientX - mouseDownPos.current.x;
    const dy = e.clientY - mouseDownPos.current.y;

    // Apply sensitivity: max displacement ~200px maps to full stick deflection
    const maxPixels = 200.0 / (sensitivity || 1.0);
    const normX = Math.max(-1.0, Math.min(1.0, dx / maxPixels));
    const normY = Math.max(-1.0, Math.min(1.0, dy / maxPixels));

    // Convert to 0-255 range (128 = center), clamp to avoid u8 overflow in Rust
    const stickX = Math.min(255, Math.max(0, Math.round(128 + normX * 127.5)));
    const stickY = Math.min(255, Math.max(0, Math.round(128 + normY * 127.5)));

    // Throttle to ~60fps
    const now = Date.now();
    if (now - lastStickSend.current < STICK_THROTTLE_MS) return;
    lastStickSend.current = now;

    onInput('stick', { stick, x: stickX, y: stickY, duration: 50 });
  }, [sensitivity, onInput]);

  const handleMouseUp = useCallback((_e) => {
    const stick = activeStick.current;
    if (!stick) return;

    // Center the stick on release
    onInput('stick', { stick, x: 128, y: 128, duration: 0 });
    activeStick.current = null;
  }, [onInput]);

  const handleContextMenu = useCallback((e) => {
    // Prevent context menu when right stick mouse is active
    if (rightEnabled) e.preventDefault();
  }, [rightEnabled]);

  // Bind/unbind mouse events when visibility or stick state changes
  useEffect(() => {
    if (!visible) {
      // Cleanup: release stick if we had one
      if (activeStick.current) {
        onInput('stick', { stick: activeStick.current, x: 128, y: 128, duration: 0 });
        activeStick.current = null;
      }
      return;
    }

    document.addEventListener('mousedown', handleMouseDown);
    document.addEventListener('mousemove', handleMouseMoveRaw);
    document.addEventListener('mouseup', handleMouseUp);
    document.addEventListener('contextmenu', handleContextMenu);

    return () => {
      document.removeEventListener('mousedown', handleMouseDown);
      document.removeEventListener('mousemove', handleMouseMoveRaw);
      document.removeEventListener('mouseup', handleMouseUp);
      document.removeEventListener('contextmenu', handleContextMenu);
    };
  }, [visible, handleMouseDown, handleMouseMoveRaw, handleMouseUp, handleContextMenu, onInput]);

  if (!visible) return null;

  return (
    <div className="controller-window-overlay" onClick={onClose}>
      <div className="controller-window-modal" onClick={e => e.stopPropagation()}>
        <div className="controller-window-header">
          <h2>Controller Window</h2>
          <button className="btn btn-close" onClick={onClose}>&times;</button>
        </div>
        <div className="controller-window-body">
          <Controller onInput={onInput} serialOpen={serialOpen} />

          {/* ── Mouse Stick Controls ─────────────────────────────── */}
          <div className="mouse-stick-controls">
            <h3>🎮 マウススティック制御</h3>
            <p className="text-muted">
              マウス移動をスティック操作に変換します。
              有効にすると、マウスのドラッグでスティックを倒せます。
            </p>

            <div className="mouse-stick-toggles">
              <label className="toggle-label">
                <input
                  type="checkbox"
                  checked={leftEnabled}
                  onChange={(e) => toggleMouseStick('left', e.target.checked)}
                />
                <span>左スティック (左クリック + ドラッグ)</span>
              </label>
              <label className="toggle-label">
                <input
                  type="checkbox"
                  checked={rightEnabled}
                  onChange={(e) => toggleMouseStick('right', e.target.checked)}
                />
                <span>右スティック (右クリック + ドラッグ)</span>
              </label>
            </div>

            <div className="sensitivity-control">
              <label>
                感度: {sensitivity.toFixed(1)}x
                <input
                  type="range"
                  min="0.25"
                  max="4.0"
                  step="0.25"
                  value={sensitivity}
                  onChange={(e) => handleSensitivityChange(e.target.value)}
                />
              </label>
            </div>

            {(leftEnabled || rightEnabled) && (
              <div className="mouse-stick-active-hint">
                <span className="badge badge-success">
                  マウススティック アクティブ
                </span>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
