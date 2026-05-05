import React, { useState, useEffect } from 'react';

export default function Scripts({ api, onStart, onStop, activeCommand }) {
  const [commands, setCommands] = useState([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    loadCommands();
  }, []);

  const loadCommands = async () => {
    try {
      setLoading(true);
      const data = await api.getCommands();
      setCommands(data.commands || []);
    } catch (err) {
      console.error('Failed to load commands:', err);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="scripts">
      <div className="panel-header">
        <h2>📜 スクリプト一覧</h2>
        <button className="btn btn-secondary" onClick={loadCommands}>
          🔄 更新
        </button>
      </div>

      {activeCommand && (
        <div className="active-banner">
          <span className="pulse-dot"></span>
          <span>実行中: {activeCommand}</span>
          <button className="btn btn-danger" onClick={onStop}>
            ⏹ 停止
          </button>
        </div>
      )}

      {loading ? (
        <div className="loading-state">
          <div className="spinner"></div>
          <p>読み込み中...</p>
        </div>
      ) : commands.length === 0 ? (
        <div className="empty-state">
          <span className="empty-icon">📂</span>
          <p>スクリプトが見つかりません</p>
          <p className="text-muted">Scripts/ ディレクトリに .py ファイルを配置してください</p>
        </div>
      ) : (
        <div className="command-list">
          {commands.map((cmd) => (
            <div key={cmd.name} className="command-card">
              <div className="command-info">
                <h3>{cmd.name}</h3>
                {cmd.description && (
                  <p className="command-desc">{cmd.description}</p>
                )}
                <span className="command-path">{cmd.path}</span>
              </div>
              <button
                className="btn btn-primary"
                onClick={() => onStart(cmd.name)}
                disabled={activeCommand === cmd.name}
              >
                {activeCommand === cmd.name ? '▶ 実行中' : '▶ 開始'}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
