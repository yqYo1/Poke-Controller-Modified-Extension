import { describe, expect, it, vi } from 'vitest';

import type { SettingsWriteValues } from './api';
import type { InputRoute, InputView, RoutedMessageSubscriber } from './input';
import type { MediaView } from './media';
import type { RealtimeView } from './realtime';
import { ApplicationRuntime, type RuntimeBundle, type RuntimeView } from './runtime';
import type { SettingsWriteResult } from './settings';
import { settingsSnapshot, stateSnapshot } from './test-fixtures';
import type { ServerMessage, SettingsSnapshot } from './wire';

interface FakeBundle {
  readonly bundle: RuntimeBundle;
  readonly emitFrame: (frame: ArrayBuffer) => void;
  readonly emitInput: (view: InputView) => void;
  readonly emitMedia: (view: MediaView) => void;
  readonly emitMessage: (route: InputRoute, message: ServerMessage) => void;
  readonly emitRealtime: (view: RealtimeView) => void;
  readonly inputDisconnected: ReturnType<typeof vi.fn>;
  readonly lifecycle: string[];
  readonly settingsWrite: ReturnType<
    typeof vi.fn<(values: SettingsWriteValues) => Promise<SettingsWriteResult>>
  >;
}

function fakeBundle(): FakeBundle {
  const lifecycle: string[] = [];
  const realtimeSubscribers = new Set<(view: RealtimeView) => void>();
  const mediaSubscribers = new Set<(view: MediaView) => void>();
  const inputSubscribers = new Set<(view: InputView) => void>();
  const messageSubscribers = new Set<RoutedMessageSubscriber>();
  const frameSubscribers = new Set<(frame: ArrayBuffer | Blob) => void>();
  const settingsSubscribers = new Set<(snapshot: SettingsSnapshot | null) => void>();
  const inputDisconnected = vi.fn();
  const settingsWrite = vi.fn<(values: SettingsWriteValues) => Promise<SettingsWriteResult>>();

  const realtime = {
    reconnectNow: vi.fn(),
    start: () => lifecycle.push('realtime.start'),
    stop: () => lifecycle.push('realtime.stop'),
    subscribe(subscriber: (view: RealtimeView) => void) {
      realtimeSubscribers.add(subscriber);
      return () => realtimeSubscribers.delete(subscriber);
    }
  };
  const media = {
    markVideoActivity: vi.fn(),
    onFallbackFrame(subscriber: (frame: ArrayBuffer | Blob) => void) {
      frameSubscribers.add(subscriber);
      return () => frameSubscribers.delete(subscriber);
    },
    onMessage(subscriber: RoutedMessageSubscriber) {
      messageSubscribers.add(subscriber);
      return () => messageSubscribers.delete(subscriber);
    },
    reconnectWebRtc: vi.fn(),
    send: vi.fn().mockReturnValue(true),
    start: () => lifecycle.push('media.start'),
    stop: () => lifecycle.push('media.stop'),
    subscribe(subscriber: (view: MediaView) => void) {
      mediaSubscribers.add(subscriber);
      return () => mediaSubscribers.delete(subscriber);
    }
  };
  const input = {
    neutralize: vi.fn(),
    setGamepadButton: vi.fn(),
    setGamepadHat: vi.fn(),
    setGamepadStick: vi.fn(),
    setGamepadTouch: vi.fn(),
    setKeyboardKey: vi.fn(),
    start: () => lifecycle.push('input.start'),
    stop: () => lifecycle.push('input.stop'),
    subscribe(subscriber: (view: InputView) => void) {
      inputSubscribers.add(subscriber);
      return () => inputSubscribers.delete(subscriber);
    },
    transportDisconnected: inputDisconnected
  };
  const settings = {
    acceptSnapshot(snapshot: SettingsSnapshot) {
      for (const subscriber of settingsSubscribers) {
        subscriber(snapshot);
      }
    },
    subscribe(subscriber: (snapshot: SettingsSnapshot | null) => void) {
      settingsSubscribers.add(subscriber);
      subscriber(null);
      return () => settingsSubscribers.delete(subscriber);
    },
    write: settingsWrite
  };

  return {
    bundle: { input, media, realtime, settings },
    emitFrame: (frame) => {
      for (const subscriber of frameSubscribers) subscriber(frame);
    },
    emitInput: (view) => {
      for (const subscriber of inputSubscribers) subscriber(view);
    },
    emitMedia: (view) => {
      for (const subscriber of mediaSubscribers) subscriber(view);
    },
    emitMessage: (route, message) => {
      for (const subscriber of messageSubscribers) subscriber(route, message);
    },
    emitRealtime: (view) => {
      for (const subscriber of realtimeSubscribers) subscriber(view);
    },
    inputDisconnected,
    lifecycle,
    settingsWrite
  };
}

function connectedView(overrides: Partial<RealtimeView> = {}): RealtimeView {
  return {
    attempts: 0,
    lastError: null,
    latestRevision: '3',
    maxRetries: 20,
    settings: settingsSnapshot('3', { 'ui.stdout_destination': 'output_2' }),
    state: stateSnapshot('3'),
    status: 'connected',
    ...overrides
  };
}

describe('ApplicationRuntime', () => {
  it('owns startup order, visible snapshots, messages, and disconnect neutralization', () => {
    const fake = fakeBundle();
    const runtime = new ApplicationRuntime(fake.bundle);
    let view!: RuntimeView;
    runtime.subscribe((next) => {
      view = next;
    });
    const frames: ArrayBuffer[] = [];
    runtime.onFallbackFrame((frame) => {
      if (frame instanceof ArrayBuffer) frames.push(frame);
    });

    runtime.start();
    expect(fake.lifecycle).toEqual(['input.start', 'media.start', 'realtime.start']);
    fake.emitRealtime(connectedView());
    expect(view.settings?.revision).toBe('3');
    expect(view.state?.revision).toBe('3');

    fake.emitMessage('websocket', {
      data: { level: 'info', message: 'stdout\n', target: 'stdout' },
      type: 'log'
    });
    fake.emitMessage('webrtc', {
      data: { level: 'warning', message: 'panel one\n', target: 'panel1' },
      type: 'log'
    });
    fake.emitMessage('websocket', {
      data: { byte_length: 2, data: 'aGk=', encoding: 'base64' },
      type: 'serial.data'
    });
    const frame = new ArrayBuffer(4);
    fake.emitFrame(frame);

    expect(view.output1.map((line) => line.message)).toEqual(['panel one\n']);
    expect(view.output2.map((line) => line.message)).toEqual(['stdout\n']);
    expect(view.serial[0]).toMatchObject({ byteLength: 2, text: 'hi' });
    expect(frames).toEqual([frame]);

    fake.emitRealtime(connectedView({ status: 'waiting' }));
    expect(fake.inputDisconnected).toHaveBeenCalledOnce();
    runtime.stop();
    expect(fake.lifecycle.slice(-3)).toEqual(['input.stop', 'media.stop', 'realtime.stop']);
  });

  it('bounds output history and reports malformed serial envelopes', () => {
    const fake = fakeBundle();
    const runtime = new ApplicationRuntime(fake.bundle);
    let view!: RuntimeView;
    runtime.subscribe((next) => {
      view = next;
    });
    runtime.start();

    for (let index = 0; index < 2_001; index += 1) {
      fake.emitMessage('websocket', {
        data: { level: 'debug', message: String(index), target: 'panel1' },
        type: 'log'
      });
    }
    expect(view.output1).toHaveLength(2_000);
    expect(view.output1[0]?.message).toBe('1');

    fake.emitMessage('websocket', {
      data: { byte_length: 3, data: 'aGk=', encoding: 'base64' },
      type: 'serial.data'
    });
    expect(view.actionError).toContain('byte length');
  });

  it('surfaces conflict recovery and runtime apply failures from settings writes', async () => {
    const fake = fakeBundle();
    const runtime = new ApplicationRuntime(fake.bundle);
    let view!: RuntimeView;
    runtime.subscribe((next) => {
      view = next;
    });
    runtime.start();
    fake.settingsWrite.mockResolvedValue({
      recoveredRevisionConflict: true,
      snapshot: {
        ...settingsSnapshot('8'),
        apply_failures: { 'ui.fps': 'display timer rejected the new value' }
      }
    });

    await runtime.writeSettings({ 'ui.fps': 60 });

    expect(view.notice).toContain('競合');
    expect(view.actionError).toContain('display timer rejected');
  });
});
