import { afterEach, describe, expect, it, vi } from 'vitest';

import type { VisibleSnapshots } from './api';
import {
  RealtimeClient,
  type RealtimeDependencies,
  type RealtimeView,
  type SocketCloseEvent,
  type SocketMessageEvent,
  type WebSocketLike
} from './realtime';
import { settingsSnapshot, stateSnapshot } from './test-fixtures';

class FakeSocket implements WebSocketLike {
  binaryType: BinaryType = 'blob';
  closeCalls: { code?: number; reason?: string }[] = [];
  onclose: ((event: SocketCloseEvent) => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((event: SocketMessageEvent) => void) | null = null;
  onopen: (() => void) | null = null;
  readyState = 0;
  sent: string[] = [];

  open(): void {
    this.readyState = 1;
    this.onopen?.();
  }

  receive(message: unknown): void {
    const data = typeof message === 'string' ? message : JSON.stringify(message);
    this.onmessage?.({ data });
  }

  disconnect(reason = '', code = 1006, wasClean = false): void {
    this.readyState = 3;
    this.onclose?.({ code, reason, wasClean });
  }

  close(code?: number, reason?: string): void {
    this.closeCalls.push({ code, reason });
    this.disconnect(reason, code ?? 1000, code === 1000);
  }

  send(data: string): void {
    this.sent.push(data);
  }
}

interface Deferred<T> {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolvePromise: ((value: T) => void) | undefined;
  const promise = new Promise<T>((resolve) => {
    resolvePromise = resolve;
  });
  if (resolvePromise === undefined) {
    throw new Error('deferred promise resolver was not initialized');
  }
  return { promise, resolve: resolvePromise };
}

function snapshots(settingsRevision: string, stateRevision = settingsRevision): VisibleSnapshots {
  return {
    settings: settingsSnapshot(settingsRevision),
    state: stateSnapshot(stateRevision)
  };
}

function stateChange(
  revision: string,
  state: Record<string, unknown> = {},
  settings: Record<string, unknown> | null = null
): unknown {
  return {
    data: { cause: 'other', settings, state },
    revision,
    type: 'ui.state.changed'
  };
}

function makeClient(
  loadSnapshots: () => Promise<VisibleSnapshots>,
  dependencyOverrides: Partial<RealtimeDependencies> = {}
): { client: RealtimeClient; sockets: FakeSocket[]; view: () => RealtimeView } {
  const sockets: FakeSocket[] = [];
  let currentView: RealtimeView | undefined;
  const client = new RealtimeClient({
    createSocket: () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket;
    },
    loadSnapshots,
    socketUrl: () => 'ws://127.0.0.1:8020/ws',
    ...dependencyOverrides
  });
  client.subscribe((view) => {
    currentView = view;
  });
  return {
    client,
    sockets,
    view: () => {
      if (currentView === undefined) {
        throw new Error('client view was not published');
      }
      return currentView;
    }
  };
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function socketAt(sockets: readonly FakeSocket[], index: number): FakeSocket {
  const socket = sockets[index];
  if (socket === undefined) {
    throw new Error(`socket ${String(index)} was not created`);
  }
  return socket;
}

function sentAt(socket: FakeSocket, index: number): string {
  const message = socket.sent[index];
  if (message === undefined) {
    throw new Error(`message ${String(index)} was not sent`);
  }
  return message;
}

afterEach(() => {
  vi.useRealTimers();
});

describe('RealtimeClient revision synchronization', () => {
  it('buffers before snapshots and applies each domain from its own baseline', async () => {
    const pending = deferred<VisibleSnapshots>();
    const { client, sockets, view } = makeClient(() => pending.promise);

    client.start();
    const socket = socketAt(sockets, 0);
    socket.open();
    socket.receive(stateChange('3', { last_input: 'A' }));
    socket.receive(
      stateChange('2', {}, {
        apply_failures: {},
        pending_restart_values: {},
        restart_required: [],
        values: { 'websocket.reconnect_max_retries': 7 }
      })
    );
    pending.resolve(snapshots('1', '2'));
    await settle();

    expect(view().status).toBe('connected');
    expect(view().latestRevision).toBe('3');
    expect(view().settings?.revision).toBe('3');
    expect(view().settings?.values['websocket.reconnect_max_retries']).toBe(7);
    expect(view().state?.revision).toBe('3');
    expect(view().state?.last_input).toBe('A');
  });

  it('re-fetches both snapshots instead of inferring a missing revision', async () => {
    const loads = vi
      .fn<() => Promise<VisibleSnapshots>>()
      .mockResolvedValueOnce(snapshots('1', '3'))
      .mockResolvedValueOnce(snapshots('3'));
    const { client, sockets, view } = makeClient(loads);

    client.start();
    const socket = socketAt(sockets, 0);
    socket.open();
    socket.receive(stateChange('3', { last_input: 'covered' }));
    await settle();

    expect(loads).toHaveBeenCalledTimes(2);
    expect(view().status).toBe('connected');
    expect(view().latestRevision).toBe('3');
  });

  it('re-fetches after duplicate revisions', async () => {
    const pending = deferred<VisibleSnapshots>();
    const loads = vi
      .fn<() => Promise<VisibleSnapshots>>()
      .mockReturnValueOnce(pending.promise)
      .mockResolvedValueOnce(snapshots('2'));
    const { client, sockets, view } = makeClient(loads);

    client.start();
    const socket = socketAt(sockets, 0);
    socket.open();
    socket.receive(stateChange('2', { last_input: 'first' }));
    socket.receive(stateChange('2', { last_input: 'duplicate' }));
    pending.resolve(snapshots('1'));
    await settle();

    expect(loads).toHaveBeenCalledTimes(2);
    expect(view().status).toBe('connected');
    expect(view().state?.revision).toBe('2');
  });

  it('re-synchronizes a semantically incomplete command generation', async () => {
    const loads = vi
      .fn<() => Promise<VisibleSnapshots>>()
      .mockResolvedValueOnce(snapshots('1'))
      .mockResolvedValueOnce({
        settings: settingsSnapshot('2'),
        state: stateSnapshot('2', {
          command_candidates: [
            { class_name: 'Example', module_path: 'commands.example', name: 'Example', tags: [] }
          ],
          command_display_lists: { '-': [] },
          tags: []
        })
      });
    const { client, sockets, view } = makeClient(loads);

    client.start();
    const socket = socketAt(sockets, 0);
    socket.open();
    await settle();
    socket.receive(stateChange('2', { command_candidates: [] }));
    await settle();

    expect(loads).toHaveBeenCalledTimes(2);
    expect(view().status).toBe('connected');
    expect(view().state?.command_candidates[0]?.name).toBe('Example');
  });
});

describe('RealtimeClient transport lifecycle', () => {
  it('replies to ping and closes an unresponsive connection at the configured deadline', async () => {
    vi.useFakeTimers();
    const { client, sockets, view } = makeClient(() => Promise.resolve(snapshots('0')), {
      now: () => Date.now()
    });

    client.start();
    const socket = socketAt(sockets, 0);
    socket.open();
    await settle();
    socket.receive({ type: 'ping', data: { nonce: 'nonce-1' } });

    expect(JSON.parse(sentAt(socket, 0)) as unknown).toEqual({
      type: 'pong',
      data: { nonce: 'nonce-1' }
    });
    await vi.advanceTimersByTimeAsync(24_999);
    expect(socket.closeCalls).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1);
    expect(socket.closeCalls.at(-1)).toEqual({
      code: 4000,
      reason: 'heartbeat timeout'
    });
    expect(view().status).toBe('waiting');
  });

  it('stops after the configured retry count and leaves manual retry explicit', async () => {
    vi.useFakeTimers();
    const configured = {
      'websocket.reconnect_interval_sec': 1,
      'websocket.reconnect_max_retries': 2
    } as const;
    const { client, sockets, view } = makeClient(() =>
      Promise.resolve({
        settings: settingsSnapshot('0', configured),
        state: stateSnapshot('0')
      })
    );

    client.start();
    socketAt(sockets, 0).open();
    await settle();
    socketAt(sockets, 0).disconnect('network lost');
    expect(view().status).toBe('waiting');

    await vi.advanceTimersByTimeAsync(1000);
    expect(sockets).toHaveLength(2);
    socketAt(sockets, 1).disconnect('retry 1 failed');
    await vi.advanceTimersByTimeAsync(1000);
    expect(sockets).toHaveLength(3);
    socketAt(sockets, 2).disconnect('retry 2 failed');
    expect(view().status).toBe('exhausted');
    expect(view().attempts).toBe(2);

    await vi.advanceTimersByTimeAsync(10_000);
    expect(sockets).toHaveLength(3);
    client.reconnectNow();
    expect(sockets).toHaveLength(4);
    socketAt(sockets, 3).disconnect('manual retry failed');
    expect(view().status).toBe('exhausted');
    await vi.advanceTimersByTimeAsync(10_000);
    expect(sockets).toHaveLength(4);
  });

  it('cancels pending retries on explicit stop', async () => {
    vi.useFakeTimers();
    const { client, sockets, view } = makeClient(() => Promise.resolve(snapshots('0')));

    client.start();
    socketAt(sockets, 0).open();
    await settle();
    socketAt(sockets, 0).disconnect('offline');
    client.stop();
    await vi.advanceTimersByTimeAsync(60_000);

    expect(sockets).toHaveLength(1);
    expect(view().status).toBe('idle');
  });
});
