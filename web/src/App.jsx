import React, { useState, useEffect, useCallback, useRef } from 'react';
import Dashboard from './components/Dashboard.jsx';
import Controller from './components/Controller.jsx';
import Scripts from './components/Scripts.jsx';
import Settings from './components/Settings.jsx';
import NavBar from './components/NavBar.jsx';
import StatusBar from './components/StatusBar.jsx';
import { APIClient, WebSocketClient } from './api/client.js';

const API_BASE = window.location.hostname === 'localhost' || window.location.hostname === '127.0.0.1'
  ? 'http://127.0.0.1:8020'
  : `${window.location.protocol}//${window.location.host}`;

export default function App() {
  const [activeTab, setActiveTab] = useState('dashboard');
  const [connected, setConnected] = useState(false);
  const [serialOpen, setSerialOpen] = useState(false);
  const [cameraOpen, setCameraOpen] = useState(false);
  const [activeCommand, setActiveCommand] = useState(null);
  const [logs, setLogs] = useState([]);
  const [cameraFrame, setCameraFrame] = useState(null);
  const [flip, setFlip] = useState('none');
  const wsRef = useRef(null);
  const apiRef = useRef(new APIClient(API_BASE));

  const addLog = useCallback((message, level = 'info') => {
    const entry = { time: new Date().toLocaleTimeString('ja-JP'), message, level };
    setLogs(prev => [...prev.slice(-99), entry]);
  }, []);

  useEffect(() => {
    const api = apiRef.current;
    const ws = new WebSocketClient(API_BASE.replace('http', 'ws') + '/ws');
    wsRef.current = ws;

    ws.onOpen = () => {
      setConnected(true);
      addLog('WebSocket接続完了', 'success');
    };
    ws.onClose = () => {
      setConnected(false);
      addLog('WebSocket切断', 'warn');
    };
    ws.onMessage = (data) => {
      switch (data.type) {
        case 'camera.frame':
          setCameraFrame(data.payload.image);
          break;
        case 'command.start':
          setActiveCommand(data.payload.name);
          addLog(`コマンド開始: ${data.payload.name}`, 'success');
          break;
        case 'command.stop':
          setActiveCommand(null);
          addLog('コマンド停止', 'info');
          break;
        case 'command.error':
          addLog(`エラー: ${data.payload.message}`, 'error');
          break;
        case 'serial.data':
          addLog(`Serial: ${data.payload.data}`, 'info');
          break;
        default:
          addLog(`Event: ${data.type}`, 'debug');
      }
    };

    ws.connect();

    // 初期状態確認
    api.getSerialStatus().then(status => setSerialOpen(status.is_open)).catch(() => {});
    api.getCameraStatus().then(status => setCameraOpen(status.is_open)).catch(() => {});
    api.getActiveCommand().then(cmd => setActiveCommand(cmd.active ? cmd.name : null)).catch(() => {});

    // 定期ヘルスチェック
    const interval = setInterval(() => {
      api.getStatus().catch(() => setConnected(false));
    }, 5000);

    return () => {
      clearInterval(interval);
      ws.disconnect();
    };
  }, [addLog]);

  const handleSerialOpen = async (config) => {
    try {
      await apiRef.current.openSerial(config);
      setSerialOpen(true);
      addLog(`シリアルポート接続: ${config.port_name || config.port_num}`, 'success');
    } catch (err) {
      addLog(`接続失敗: ${err.message}`, 'error');
    }
  };

  const handleSerialClose = async () => {
    try {
      await apiRef.current.closeSerial();
      setSerialOpen(false);
      addLog('シリアルポート切断', 'info');
    } catch (err) {
      addLog(`切断失敗: ${err.message}`, 'error');
    }
  };

  const handleCameraOpen = async (config) => {
    try {
      await apiRef.current.openCamera(config);
      setCameraOpen(true);
      addLog('カメラ接続', 'success');
    } catch (err) {
      addLog(`カメラ接続失敗: ${err.message}`, 'error');
    }
  };

  const handleFlipChange = async (mode) => {
    try {
      await apiRef.current.updateCameraConfig({ flip: mode });
      setFlip(mode);
      addLog(`反転モード: ${mode}`, 'info');
    } catch (err) {
      addLog(`反転設定失敗: ${err.message}`, 'error');
    }
  };

  const handleCapture = async () => {
    try {
      const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
      const filename = `capture_${timestamp}.jpg`;
      const result = await apiRef.current.captureCamera(filename);
      addLog(`キャプチャ保存: ${result.path || filename}`, 'success');
    } catch (err) {
      addLog(`キャプチャ失敗: ${err.message}`, 'error');
    }
  };

  const handleCameraClose = async () => {
    try {
      await apiRef.current.closeCamera();
      setCameraOpen(false);
      setCameraFrame(null);
      setFlip('none');
      addLog('カメラ切断', 'info');
    } catch (err) {
      addLog(`カメラ切断失敗: ${err.message}`, 'error');
    }
  };

  const handleInput = async (type, params) => {
    if (!serialOpen) {
      addLog('シリアルポート未接続', 'error');
      return;
    }
    try {
      await apiRef.current.sendInput(type, params);
    } catch (err) {
      addLog(`入力エラー: ${err.message}`, 'error');
    }
  };

  const handleCommandStart = async (name) => {
    try {
      await apiRef.current.startCommand(name);
      addLog(`コマンド開始: ${name}`, 'success');
    } catch (err) {
      addLog(`コマンド開始失敗: ${err.message}`, 'error');
    }
  };

  const handleCommandStop = async () => {
    try {
      await apiRef.current.stopCommand();
      addLog('コマンド停止', 'info');
    } catch (err) {
      addLog(`停止失敗: ${err.message}`, 'error');
    }
  };

  const tabs = {
    dashboard: (
      <Dashboard
        cameraFrame={cameraFrame}
        cameraOpen={cameraOpen}
        onCameraOpen={handleCameraOpen}
        onCameraClose={handleCameraClose}
        onCapture={handleCapture}
        flip={flip}
        onFlipChange={handleFlipChange}
        logs={logs}
        activeCommand={activeCommand}
      />
    ),
    controller: (
      <Controller
        onInput={handleInput}
        serialOpen={serialOpen}
      />
    ),
    scripts: (
      <Scripts
        api={apiRef.current}
        onStart={handleCommandStart}
        onStop={handleCommandStop}
        activeCommand={activeCommand}
      />
    ),
    settings: (
      <Settings
        serialOpen={serialOpen}
        onSerialOpen={handleSerialOpen}
        onSerialClose={handleSerialClose}
        api={apiRef.current}
      />
    ),
  };

  return (
    <div className="app">
      <StatusBar connected={connected} serialOpen={serialOpen} cameraOpen={cameraOpen} />
      <main className="main-content">
        {tabs[activeTab]}
      </main>
      <NavBar activeTab={activeTab} onChange={setActiveTab} />
    </div>
  );
}
