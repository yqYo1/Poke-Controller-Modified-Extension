import React from 'react';
import Controller from './Controller.jsx';

export default function ControllerWindow({ visible, onClose, onInput, serialOpen }) {
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
        </div>
      </div>
    </div>
  );
}
