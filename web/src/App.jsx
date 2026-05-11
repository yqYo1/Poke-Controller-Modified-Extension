import React, { useState, useEffect, useCallback, useRef } from 'react';
import Dashboard from './components/Dashboard.jsx';
import Controller from './components/Controller.jsx';
import Scripts from './components/Scripts.jsx';
import Settings from './components/Settings.jsx';
import NavBar from './components/NavBar.jsx';
import StatusBar from './components/StatusBar.jsx';
import ControllerWindow from './components/ControllerWindow.jsx';
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
  const [controllerWindowOpen, setControllerWindowOpen] = useState(false);
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
      addLog('WebSocket connected', 'success');
    };
    ws.onClose = () => {
      setConnected(false);
      addLog('WebSocket disconnected', 'warn');
    };
    ws.onMessage = (data) => {
      switch (data.type) {
        case 'camera.frame':
          setCameraFrame(data.payload.image);
          break;
        case 'command.start':
          setActiveCommand(data.payload.name);
          addLog(`Command started: ${data.payload.name}`, 'success');
          break;
        case 'command.stop':
          setActiveCommand(null);
          addLog('Command stopped', 'info');
          break;
        case 'command.error':
          addLog(`Error: ${data.payload.message}`, 'error');
          break;
        case 'serial.data':
          addLog(`Serial: ${data.payload.data}`, 'info');
          break;
        default:
          addLog(`Event: ${data.type}`, 'debug');
      }
    };

    ws.connect();

    api.getSerialStatus().then(status => setSerialOpen(status.is_open)).catch(() => {});
    api.getCameraStatus().then(status => setCameraOpen(status.is_open)).catch(() => {});
    api.getActiveCommand().then(cmd => setActiveCommand(cmd.active ? cmd.name : null)).catch(() => {});

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
      addLog(`Serial port connected: ${config.port_name || config.port_num}`, 'success');
    } catch (err) {
      addLog(`Connection failed: ${err.message}`, 'error');
    }
  };

  const handleSerialClose = async () => {
    try {
      await apiRef.current.closeSerial();
      setSerialOpen(false);
      addLog('Serial port disconnected', 'info');
    } catch (err) {
      addLog(`Disconnect failed: ${err.message}`, 'error');
    }
  };

  const handleCameraOpen = async (config) => {
    try {
      await apiRef.current.openCamera(config);
      setCameraOpen(true);
      addLog('Camera connected', 'success');
    } catch (err) {
      addLog(`Camera connection failed: ${err.message}`, 'error');
    }
  };

  const handleFlipChange = async (mode) => {
    try {
      await apiRef.current.updateCameraConfig({ flip: mode });
      setFlip(mode);
      addLog(`Flip mode: ${mode}`, 'info');
    } catch (err) {
      addLog(`Flip setting failed: ${err.message}`, 'error');
    }
  };

  const handleCapture = async () => {
    try {
      const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
      const filename = `capture_${timestamp}.jpg`;
      const result = await apiRef.current.captureCamera(filename);
      addLog(`Capture saved: ${result.path || filename}`, 'success');
    } catch (err) {
      addLog(`Capture failed: ${err.message}`, 'error');
    }
  };

  const handleCameraClose = async () => {
    try {
      await apiRef.current.closeCamera();
      setCameraOpen(false);
      setCameraFrame(null);
      setFlip('none');
      addLog('Camera disconnected', 'info');
    } catch (err) {
      addLog(`Camera disconnect failed: ${err.message}`, 'error');
    }
  };

  const handleInput = async (type, params) => {
    if (!serialOpen) {
      addLog('Serial port not connected', 'error');
      return;
    }
    try {
      await apiRef.current.sendInput(type, params);
    } catch (err) {
      addLog(`Input error: ${err.message}`, 'error');
    }
  };

  const handleCommandStart = async (name) => {
    try {
      await apiRef.current.startCommand(name);
      addLog(`Command started: ${name}`, 'success');
    } catch (err) {
      addLog(`Command start failed: ${err.message}`, 'error');
    }
  };

  const handleCommandStop = async () => {
    try {
      await apiRef.current.stopCommand();
      addLog('Command stopped', 'info');
    } catch (err) {
      addLog(`Stop failed: ${err.message}`, 'error');
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
        onControllerWindowOpen={() => setControllerWindowOpen(true)}
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
      <ControllerWindow
        visible={controllerWindowOpen}
        onClose={() => setControllerWindowOpen(false)}
        onInput={handleInput}
        serialOpen={serialOpen}
      />
    </div>
  );
}
