import React, { useState, useEffect, useCallback } from 'react';

const SHORTCUT_COUNT = 4;
const SHORTCUT_STORAGE_KEY = 'pokecon_shortcuts';

export default function Scripts({ api, onStart, onStop, activeCommand }) {
  const [commands, setCommands] = useState([]);
  const [loading, setLoading] = useState(true);
  const [filterTab, setFilterTab] = useState('all');
  const [filterPy, setFilterPy] = useState('');
  const [filterMcu, setFilterMcu] = useState('');
  const [shortcuts, setShortcuts] = useState(() => {
    try {
      const saved = localStorage.getItem(SHORTCUT_STORAGE_KEY);
      return saved ? JSON.parse(saved) : Array(SHORTCUT_COUNT).fill(null);
    } catch {
      return Array(SHORTCUT_COUNT).fill(null);
    }
  });
  const [profiles, setProfiles] = useState([]);
  const [activeProfile, setActiveProfile] = useState('');
  const [commandFilter, setCommandFilter] = useState('');

  // Load commands and profiles on mount
  useEffect(() => {
    loadCommands();
    loadProfiles();
  }, []);

  // Keyboard shortcuts: F5 reload, F6 start, ESC stop
  useEffect(() => {
    const handleKeyDown = (e) => {
      const tag = e.target.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;

      switch (e.key) {
        case 'F5':
          e.preventDefault();
          loadCommands();
          break;
        case 'F6':
          e.preventDefault();
          if (!activeCommand && commands.length > 0) {
            onStart(commands[0].name);
          }
          break;
        case 'Escape':
        case 'Esc':
          if (activeCommand) {
            onStop();
          }
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [activeCommand, commands, onStart, onStop]);

  const loadCommands = useCallback(async () => {
    try {
      setLoading(true);
      const data = await api.getCommands();
      setCommands(data.commands || []);
    } catch (err) {
      console.error('Failed to load commands:', err);
    } finally {
      setLoading(false);
    }
  }, [api]);

  const loadProfiles = useCallback(async () => {
    try {
      const data = await api.getProfiles();
      setProfiles(data.profiles || []);
      if (data.active) {
        setActiveProfile(data.active);
      }
    } catch (err) {
      console.error('Failed to load profiles:', err);
    }
  }, [api]);

  // Filter commands by tab and search text
  const filteredCommands = commands.filter((cmd) => {
    if (filterTab === 'py') {
      const isPy = cmd.path.endsWith('.py') || cmd.path.endsWith('.pyw');
      if (!isPy) return false;
    } else if (filterTab === 'mcu') {
      const isMcu = cmd.path.endsWith('.lua') || cmd.path.endsWith('.mcuscript');
      if (!isMcu) return false;
    }

    const filterText = filterTab === 'py' ? filterPy : filterTab === 'mcu' ? filterMcu : commandFilter;
    if (!filterText) return true;

    const lower = filterText.toLowerCase();
    return (
      cmd.name.toLowerCase().includes(lower) ||
      (cmd.description || '').toLowerCase().includes(lower) ||
      cmd.path.toLowerCase().includes(lower)
    );
  });

  // Shortcut handlers
  const handleShortcutClick = (idx) => {
    const cmdName = shortcuts[idx];
    if (cmdName) {
      onStart(cmdName);
    }
  };

  const handleShortcutAssign = (idx) => {
    const available = commands.filter((c) => c.name !== activeCommand);
    if (available.length === 0) return;

    const current = shortcuts[idx];
    const currentIdx = available.findIndex((c) => c.name === current);
    const nextIdx = (currentIdx + 1) % available.length;
    const nextCmd = available[nextIdx].name;

    const newShortcuts = [...shortcuts];
    newShortcuts[idx] = nextCmd;
    setShortcuts(newShortcuts);
    localStorage.setItem(SHORTCUT_STORAGE_KEY, JSON.stringify(newShortcuts));
  };

  const handleShortcutRightClick = (e, idx) => {
    e.preventDefault();
    const newShortcuts = [...shortcuts];
    newShortcuts[idx] = null;
    setShortcuts(newShortcuts);
    localStorage.setItem(SHORTCUT_STORAGE_KEY, JSON.stringify(newShortcuts));
  };

  // Profile handler
  const handleProfileChange = async (name) => {
    setActiveProfile(name);
    try {
      await api.setProfile(name);
    } catch (err) {
      console.error('Failed to set profile:', err);
    }
  };

  // Reload (rescan) handler
  const handleReload = async () => {
    try {
      setLoading(true);
      const data = await api.reloadCommands();
      setCommands(data.commands || []);
    } catch (err) {
      console.error('Failed to reload commands:', err);
    } finally {
      setLoading(false);
    }
  };

  const getShortcutTitle = (i) => {
    if (shortcuts[i]) {
      return '実行: ' + shortcuts[i] + '\nShift+Click: 割り当て変更\n右クリック: クリア';
    }
    return 'Shift+Click: コマンドを割り当て';
  };

  return (
    <div className="scripts">
      {/* Active command banner */}
      {activeCommand && (
        <div className="active-banner">
          <span className="pulse-dot"></span>
          <span style={{ flex: 1 }}>{'実行中'}: {activeCommand}</span>
          <button className="btn btn-danger" onClick={onStop}>
            {'⏹'} {'停止'}
          </button>
        </div>
      )}

      {/* Profile selector panel */}
      <div className="panel">
        <div className="panel-header">
          <h2>{'📋'} {'プロファイル'}</h2>
        </div>
        <div style={{ padding: '8px 12px' }}>
          <div className="profile-row">
            <span className="profile-label">{'プロファイル'}:</span>
            <select
              className="profile-select"
              value={activeProfile}
              onChange={(e) => handleProfileChange(e.target.value)}
            >
              <option value="">{'選択してください'}</option>
              {profiles.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}{p.description ? ' - ' + p.description : ''}
                </option>
              ))}
            </select>
            <button className="btn btn-secondary" onClick={loadProfiles} title={'リロード'}>
              {'🔄'}
            </button>
          </div>
        </div>
      </div>

      {/* Shortcut buttons panel */}
      <div className="panel">
        <div className="panel-header">
          <h2>{'⚡'} {'ショートカット'}</h2>
        </div>
        <div style={{ padding: '8px 12px' }}>
          <div className="shortcut-row">
            {Array.from({ length: SHORTCUT_COUNT }, (_, i) => (
              <button
                key={i}
                className="shortcut-btn"
                onClick={(e) => {
                  if (e.shiftKey) {
                    handleShortcutAssign(i);
                  } else {
                    handleShortcutClick(i);
                  }
                }}
                onContextMenu={(e) => handleShortcutRightClick(e, i)}
                disabled={!shortcuts[i] || activeCommand === shortcuts[i]}
                title={getShortcutTitle(i)}
              >
                <span className="shortcut-num">#{i + 1}</span>
                <span className="shortcut-name">
                  {shortcuts[i] || '---'}
                </span>
              </button>
            ))}
          </div>
          <div className="shortcut-hints">
            <span className="shortcut-hint"><kbd>F5</kbd> {'リロード'}</span>
            <span className="shortcut-hint"><kbd>F6</kbd> {'開始'}</span>
            <span className="shortcut-hint"><kbd>ESC</kbd> {'停止'}</span>
            <span className="shortcut-hint"><kbd>Shift</kbd>+Click {'割り当て'}</span>
            <span className="shortcut-hint">{'右Click'} {'クリア'}</span>
          </div>
        </div>
      </div>

      {/* Command list with filters panel */}
      <div className="panel">
        <div className="panel-header">
          <h2>{'📜'} {'スクリプト一覧'}</h2>
          <div style={{ display: 'flex', gap: '6px' }}>
            <button className="btn btn-secondary" onClick={loadCommands} title={'F5 ' + 'リロード'}>
              {'🔄'} {'更新'}
            </button>
            <button className="btn btn-secondary" onClick={handleReload} title={'スキャンして再読み込み'}>
              {'📂'} {'再スキャン'}
            </button>
          </div>
        </div>

        <div style={{ padding: '8px 12px 0' }}>
          {/* Filter tabs: All / Python / MCU */}
          <div className="filter-tabs">
            <button
              className={'filter-tab ' + (filterTab === 'all' ? 'active' : '')}
              onClick={() => setFilterTab('all')}
            >
              {'📋'} {'すべて'}
            </button>
            <button
              className={'filter-tab ' + (filterTab === 'py' ? 'active' : '')}
              onClick={() => setFilterTab('py')}
            >
              {'🐍'} Python
            </button>
            <button
              className={'filter-tab ' + (filterTab === 'mcu' ? 'active' : '')}
              onClick={() => setFilterTab('mcu')}
            >
              {'⚙️'} MCU
            </button>
          </div>

          {/* Filter text inputs */}
          {filterTab === 'all' && (
            <div className="filter-row">
              <div className="filter-group">
                <span className="filter-label">{'フィルター'}</span>
                <input
                  type="text"
                  className="filter-input"
                  placeholder={'コマンドを検索...'}
                  value={commandFilter}
                  onChange={(e) => setCommandFilter(e.target.value)}
                />
              </div>
            </div>
          )}
          {filterTab === 'py' && (
            <div className="filter-row">
              <div className="filter-group">
                <span className="filter-label">Python {'フィルター'}</span>
                <input
                  type="text"
                  className="filter-input"
                  placeholder={'Pythonコマンドを検索...'}
                  value={filterPy}
                  onChange={(e) => setFilterPy(e.target.value)}
                />
              </div>
            </div>
          )}
          {filterTab === 'mcu' && (
            <div className="filter-row">
              <div className="filter-group">
                <span className="filter-label">MCU {'フィルター'}</span>
                <input
                  type="text"
                  className="filter-input"
                  placeholder={'MCUコマンドを検索...'}
                  value={filterMcu}
                  onChange={(e) => setFilterMcu(e.target.value)}
                />
              </div>
            </div>
          )}
        </div>

        {/* Command cards */}
        <div style={{ padding: '8px 12px 12px' }}>
          {loading ? (
            <div className="loading-state">
              <div className="spinner"></div>
              <p>{'読み込み中...'}</p>
            </div>
          ) : filteredCommands.length === 0 ? (
            <div className="empty-state">
              <span className="empty-icon">{'📂'}</span>
              <p>{'スクリプトが見つかりません'}</p>
              <p className="text-muted">{'Scripts/'} {'ディレクトリに'} .py/.lua {'ファイルを配置してください'}</p>
            </div>
          ) : (
            <div className="command-list">
              {filteredCommands.map((cmd) => {
                const cmdType = cmd.path.endsWith('.lua') ? 'MCU' : 'Python';
                return (
                  <div key={cmd.name} className="command-card">
                    <div className="command-info">
                      <h3>
                        <span style={{ marginRight: '6px' }}>
                          {cmdType === 'Python' ? '🐍' : '⚙️'}
                        </span>
                        {cmd.name}
                      </h3>
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
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
