import React from 'react';

export default function NavBar({ activeTab, onChange }) {
  const tabs = [
    { id: 'dashboard', label: 'ダッシュボード', icon: '📊' },
    { id: 'controller', label: 'コントローラー', icon: '🎮' },
    { id: 'scripts', label: 'スクリプト', icon: '📜' },
    { id: 'settings', label: '設定', icon: '⚙️' },
  ];

  return (
    <nav className="nav-bar">
      {tabs.map(tab => (
        <button
          key={tab.id}
          className={`nav-item ${activeTab === tab.id ? 'active' : ''}`}
          onClick={() => onChange(tab.id)}
        >
          <span className="nav-icon">{tab.icon}</span>
          <span className="nav-label">{tab.label}</span>
        </button>
      ))}
    </nav>
  );
}
