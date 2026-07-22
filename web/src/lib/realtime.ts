import { loadVisibleSnapshots, type VisibleSnapshots } from './api';
import type { components } from './generated/api';
import {
  compareRevisions,
  incrementRevision,
  maximumRevision,
  minimumRevision,
  type DecimalString
} from './revision';
import {
  parseServerMessage,
  parseSettingsSnapshot,
  parseStateSnapshot,
  serializeClientMessage,
  type ClientMessage,
  type ServerMessage,
  type SettingsSnapshot,
  type StateSnapshot
} from './wire';

type RevisionedStateChange = components['schemas']['RevisionedStateChange'];
type SettingsChange = components['schemas']['SettingsChange'];
type StatePatch = components['schemas']['StatePatch'];

export type ConnectionStatus =
  | 'idle'
  | 'connecting'
  | 'synchronizing'
  | 'connected'
  | 'waiting'
  | 'exhausted';

export interface RealtimeView {
  readonly attempts: number;
  readonly lastError: string | null;
  readonly latestRevision: DecimalString | null;
  readonly maxRetries: number;
  readonly settings: SettingsSnapshot | null;
  readonly state: StateSnapshot | null;
  readonly status: ConnectionStatus;
}

export interface SocketCloseEvent {
  readonly code: number;
  readonly reason: string;
  readonly wasClean: boolean;
}

export interface SocketMessageEvent {
  readonly data: unknown;
}

export interface WebSocketLike {
  binaryType: BinaryType;
  readonly readyState: number;
  onclose: ((event: SocketCloseEvent) => void) | null;
  onerror: (() => void) | null;
  onmessage: ((event: SocketMessageEvent) => void) | null;
  onopen: (() => void) | null;
  close(code?: number, reason?: string): void;
  send(data: string): void;
}

export interface RealtimeDependencies {
  readonly clearTimeout: (timer: ReturnType<typeof setTimeout>) => void;
  readonly createSocket: (url: string) => WebSocketLike;
  readonly loadSnapshots: () => Promise<VisibleSnapshots>;
  readonly now: () => number;
  readonly setTimeout: (
    callback: () => void,
    delayMilliseconds: number
  ) => ReturnType<typeof setTimeout>;
  readonly socketUrl: () => string;
}

type ViewSubscriber = (view: RealtimeView) => void;
type MessageSubscriber = (message: ServerMessage) => void;
type FrameSubscriber = (frame: ArrayBuffer | Blob) => void;

const DEFAULT_RECONNECT_INTERVAL_SECONDS = 3;
const DEFAULT_RECONNECT_MAX_RETRIES = 20;
const DEFAULT_PING_INTERVAL_SECONDS = 15;
const DEFAULT_PONG_TIMEOUT_SECONDS = 10;
const MAX_SYNCHRONIZATION_PASSES = 3;
const SOCKET_OPEN = 1;

function defaultSocketUrl(): string {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}/ws`;
}

const defaultDependencies: RealtimeDependencies = {
  clearTimeout: (timer) => {
    clearTimeout(timer);
  },
  createSocket: (url) => new WebSocket(url) as WebSocketLike,
  loadSnapshots: loadVisibleSnapshots,
  now: () => performance.now(),
  setTimeout: (callback, delayMilliseconds) => setTimeout(callback, delayMilliseconds),
  socketUrl: defaultSocketUrl
};

function errorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : 'realtime transport failed';
  return message.slice(0, 512);
}

function hasOwn(object: object, property: PropertyKey): boolean {
  return Object.prototype.hasOwnProperty.call(object, property);
}

function validateCommandGeneration(patch: StatePatch): void {
  const generationFields = [
    hasOwn(patch, 'command_candidates'),
    hasOwn(patch, 'command_display_lists'),
    hasOwn(patch, 'tags')
  ];
  const count = generationFields.filter(Boolean).length;
  if (count !== 0 && count !== generationFields.length) {
    throw new Error('command display generation is incomplete');
  }
}

function applyStateChange(
  snapshot: StateSnapshot,
  change: RevisionedStateChange
): StateSnapshot {
  validateCommandGeneration(change.data.state);
  const candidate: unknown = {
    ...snapshot,
    ...change.data.state,
    revision: change.revision
  };
  return parseStateSnapshot(candidate);
}

function applySettingsChange(
  snapshot: SettingsSnapshot,
  change: SettingsChange | null,
  revision: DecimalString
): SettingsSnapshot {
  if (change === null) {
    return parseSettingsSnapshot({ ...snapshot, revision });
  }

  const restartRequired = new Set(change.restart_required);
  const pendingRestartValues = Object.fromEntries(
    Object.entries({
      ...snapshot.pending_restart_values,
      ...change.pending_restart_values
    }).filter(([settingId]) => restartRequired.has(settingId))
  );

  return parseSettingsSnapshot({
    ...snapshot,
    apply_failures: change.apply_failures,
    pending_restart_values: pendingRestartValues,
    restart_required: change.restart_required,
    revision,
    values: { ...snapshot.values, ...change.values }
  });
}

interface AppliedSnapshots extends VisibleSnapshots {
  readonly latestRevision: DecimalString;
}

function applyBufferedChanges(
  snapshots: VisibleSnapshots,
  buffered: readonly RevisionedStateChange[]
): AppliedSnapshots {
  const ordered = [...buffered].sort((left, right) =>
    compareRevisions(left.revision, right.revision)
  );
  for (let index = 1; index < ordered.length; index += 1) {
    const previous = ordered[index - 1];
    const current = ordered[index];
    if (
      previous !== undefined &&
      current !== undefined &&
      compareRevisions(previous.revision, current.revision) === 0
    ) {
      throw new Error(`duplicate buffered revision ${current.revision}`);
    }
  }

  const floor = minimumRevision(snapshots.settings.revision, snapshots.state.revision);
  let ceiling = maximumRevision(snapshots.settings.revision, snapshots.state.revision);
  const relevant = ordered.filter((change) => compareRevisions(change.revision, floor) > 0);
  const lastBuffered = relevant.at(-1);
  if (lastBuffered !== undefined) {
    ceiling = maximumRevision(ceiling, lastBuffered.revision);
  }

  let expected = incrementRevision(floor);
  let settings = snapshots.settings;
  let state = snapshots.state;
  for (const change of relevant) {
    if (compareRevisions(change.revision, expected) !== 0) {
      throw new Error(`buffered revision gap before ${change.revision}`);
    }
    if (compareRevisions(change.revision, settings.revision) > 0) {
      settings = applySettingsChange(settings, change.data.settings, change.revision);
    }
    if (compareRevisions(change.revision, state.revision) > 0) {
      state = applyStateChange(state, change);
    }
    expected = incrementRevision(change.revision);
  }

  const covered = relevant.at(-1)?.revision ?? floor;
  if (compareRevisions(covered, ceiling) < 0) {
    throw new Error(`buffered revision gap after ${covered}`);
  }
  return { latestRevision: ceiling, settings, state };
}

export class RealtimeClient {
  private readonly dependencies: RealtimeDependencies;
  private readonly frameSubscribers = new Set<FrameSubscriber>();
  private readonly messageSubscribers = new Set<MessageSubscriber>();
  private readonly viewSubscribers = new Set<ViewSubscriber>();
  private bufferedChanges: RevisionedStateChange[] = [];
  private bufferInvalid = false;
  private heartbeatTimer: ReturnType<typeof setTimeout> | undefined;
  private lastPingAt = 0;
  private manualAttempt = false;
  private reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  private socket: WebSocketLike | undefined;
  private stopped = true;
  private synchronizing = false;
  private view: RealtimeView = {
    attempts: 0,
    lastError: null,
    latestRevision: null,
    maxRetries: DEFAULT_RECONNECT_MAX_RETRIES,
    settings: null,
    state: null,
    status: 'idle'
  };

  constructor(dependencies: Partial<RealtimeDependencies> = {}) {
    this.dependencies = { ...defaultDependencies, ...dependencies };
  }

  subscribe(subscriber: ViewSubscriber): () => void {
    this.viewSubscribers.add(subscriber);
    subscriber(this.view);
    return () => {
      this.viewSubscribers.delete(subscriber);
    };
  }

  onMessage(subscriber: MessageSubscriber): () => void {
    this.messageSubscribers.add(subscriber);
    return () => {
      this.messageSubscribers.delete(subscriber);
    };
  }

  onFrame(subscriber: FrameSubscriber): () => void {
    this.frameSubscribers.add(subscriber);
    return () => {
      this.frameSubscribers.delete(subscriber);
    };
  }

  start(): void {
    if (!this.stopped) {
      return;
    }
    this.stopped = false;
    this.updateView({ attempts: 0, lastError: null });
    this.connect(false);
  }

  stop(): void {
    this.stopped = true;
    this.clearReconnectTimer();
    this.clearHeartbeatTimer();
    const socket = this.socket;
    this.socket = undefined;
    if (socket !== undefined && socket.readyState < 2) {
      socket.close(1000, 'client stopped');
    }
    this.bufferedChanges = [];
    this.synchronizing = false;
    this.updateView({ status: 'idle' });
  }

  reconnectNow(): void {
    if (this.stopped) {
      this.stopped = false;
    }
    this.clearReconnectTimer();
    this.clearHeartbeatTimer();
    const previous = this.socket;
    this.socket = undefined;
    if (previous !== undefined && previous.readyState < 2) {
      previous.close(1000, 'manual reconnect');
    }
    this.updateView({ attempts: this.view.attempts + 1, lastError: null });
    this.connect(true);
  }

  send(message: ClientMessage): boolean {
    const socket = this.socket;
    if (socket?.readyState !== SOCKET_OPEN) {
      return false;
    }
    socket.send(serializeClientMessage(message));
    return true;
  }

  private updateView(update: Partial<RealtimeView>): void {
    this.view = { ...this.view, ...update };
    for (const subscriber of this.viewSubscribers) {
      subscriber(this.view);
    }
  }

  private connect(manual: boolean): void {
    if (this.stopped) {
      return;
    }
    this.manualAttempt = manual;
    this.bufferedChanges = [];
    this.bufferInvalid = false;
    this.synchronizing = true;
    this.updateView({ status: 'connecting' });

    let socket: WebSocketLike;
    try {
      socket = this.dependencies.createSocket(this.dependencies.socketUrl());
    } catch (error: unknown) {
      this.connectionFailed(errorMessage(error), manual);
      return;
    }
    this.socket = socket;
    socket.binaryType = 'arraybuffer';
    socket.onopen = () => {
      void this.handleOpen(socket);
    };
    socket.onmessage = (event) => {
      this.handleMessage(socket, event);
    };
    socket.onerror = () => {
      if (this.socket === socket) {
        this.updateView({ lastError: 'WebSocket transport error' });
      }
    };
    socket.onclose = (event) => {
      this.handleClose(socket, event, manual);
    };
  }

  private async handleOpen(socket: WebSocketLike): Promise<void> {
    if (this.socket !== socket || this.stopped) {
      return;
    }
    this.lastPingAt = this.dependencies.now();
    this.updateView({ status: 'synchronizing' });
    this.scheduleHeartbeat();
    try {
      await this.synchronize(socket);
    } catch (error: unknown) {
      if (this.socket === socket) {
        this.updateView({ lastError: errorMessage(error) });
        socket.close(1011, 'snapshot synchronization failed');
      }
      return;
    }
    if (!this.isActiveOpenSocket(socket)) {
      return;
    }
    this.manualAttempt = false;
    this.updateView({ attempts: 0, lastError: null, status: 'connected' });
    this.scheduleHeartbeat();
  }

  private async synchronize(socket: WebSocketLike): Promise<void> {
    this.synchronizing = true;
    for (let pass = 0; pass < MAX_SYNCHRONIZATION_PASSES; pass += 1) {
      const snapshots = await this.dependencies.loadSnapshots();
      if (this.socket !== socket || this.stopped) {
        return;
      }

      const buffered = this.bufferedChanges;
      const invalid = this.bufferInvalid;
      this.bufferedChanges = [];
      this.bufferInvalid = false;
      if (invalid) {
        continue;
      }

      try {
        const applied = applyBufferedChanges(snapshots, buffered);
        this.synchronizing = false;
        this.updateView({
          latestRevision: applied.latestRevision,
          maxRetries:
            applied.settings.values['websocket.reconnect_max_retries'],
          settings: applied.settings,
          state: applied.state
        });
        return;
      } catch (error: unknown) {
        if (pass === MAX_SYNCHRONIZATION_PASSES - 1) {
          throw error;
        }
      }
    }
    throw new Error('snapshot synchronization did not converge');
  }

  private handleMessage(socket: WebSocketLike, event: SocketMessageEvent): void {
    if (this.socket !== socket || this.stopped) {
      return;
    }
    if (event.data instanceof ArrayBuffer || event.data instanceof Blob) {
      for (const subscriber of this.frameSubscribers) {
        subscriber(event.data);
      }
      return;
    }
    if (typeof event.data !== 'string') {
      this.handleInvalidMessage('unsupported WebSocket message payload');
      return;
    }

    let message: ServerMessage;
    try {
      message = parseServerMessage(event.data);
    } catch (error: unknown) {
      this.handleInvalidMessage(errorMessage(error));
      return;
    }

    if (message.type === 'ui.state.changed') {
      this.handleStateChange(message);
      return;
    }
    if (message.type === 'ping') {
      this.lastPingAt = this.dependencies.now();
      if (socket.readyState === SOCKET_OPEN) {
        socket.send(serializeClientMessage({ type: 'pong', data: message.data }));
      }
      this.scheduleHeartbeat();
    }
    for (const subscriber of this.messageSubscribers) {
      subscriber(message);
    }
  }

  private handleInvalidMessage(message: string): void {
    this.updateView({ lastError: message });
    if (this.synchronizing) {
      this.bufferInvalid = true;
      return;
    }

    this.synchronizing = true;
    this.bufferedChanges = [];
    this.bufferInvalid = false;
    this.updateView({ status: 'synchronizing' });
    const socket = this.socket;
    if (socket !== undefined) {
      void this.synchronize(socket).then(
        () => {
          if (this.socket === socket && socket.readyState === SOCKET_OPEN) {
            this.updateView({ lastError: null, status: 'connected' });
          }
        },
        (error: unknown) => {
          if (this.socket === socket) {
            this.updateView({ lastError: errorMessage(error) });
            socket.close(1011, 'protocol resynchronization failed');
          }
        }
      );
    }
  }

  private handleStateChange(change: RevisionedStateChange): void {
    if (this.synchronizing) {
      this.bufferedChanges.push(change);
      return;
    }
    const settings = this.view.settings;
    const state = this.view.state;
    const latestRevision = this.view.latestRevision;
    if (settings === null || state === null || latestRevision === null) {
      this.handleInvalidMessage('state change arrived without a synchronized baseline');
      return;
    }
    if (compareRevisions(change.revision, incrementRevision(latestRevision)) !== 0) {
      this.bufferedChanges = [change];
      this.handleInvalidMessage(`live revision gap at ${change.revision}`);
      return;
    }

    try {
      const nextSettings = applySettingsChange(settings, change.data.settings, change.revision);
      const nextState = applyStateChange(state, change);
      this.updateView({
        latestRevision: change.revision,
        maxRetries: nextSettings.values['websocket.reconnect_max_retries'],
        settings: nextSettings,
        state: nextState
      });
      if (
        change.data.settings !== null &&
        (hasOwn(change.data.settings.values, 'websocket.ping_interval_sec') ||
          hasOwn(change.data.settings.values, 'websocket.pong_timeout_sec'))
      ) {
        this.lastPingAt = this.dependencies.now();
        this.scheduleHeartbeat();
      }
    } catch (error: unknown) {
      this.handleInvalidMessage(errorMessage(error));
    }
  }

  private handleClose(
    socket: WebSocketLike,
    event: SocketCloseEvent,
    manual: boolean
  ): void {
    if (this.socket !== socket) {
      return;
    }
    this.socket = undefined;
    this.synchronizing = false;
    this.clearHeartbeatTimer();
    if (this.stopped) {
      return;
    }

    const reason = event.reason.trim();
    const fallback = event.wasClean
      ? `WebSocket closed (${String(event.code)})`
      : `WebSocket disconnected (${String(event.code)})`;
    this.connectionFailed(reason === '' ? fallback : reason, manual || this.manualAttempt);
  }

  private connectionFailed(message: string, manual: boolean): void {
    this.updateView({ lastError: message });
    if (manual) {
      this.updateView({ status: 'exhausted' });
      return;
    }

    const maximum = this.reconnectMaximum();
    if (maximum === 0 || this.view.attempts >= maximum) {
      this.updateView({ maxRetries: maximum, status: 'exhausted' });
      return;
    }

    this.updateView({ maxRetries: maximum, status: 'waiting' });
    this.clearReconnectTimer();
    this.reconnectTimer = this.dependencies.setTimeout(() => {
      this.reconnectTimer = undefined;
      this.updateView({ attempts: this.view.attempts + 1 });
      this.connect(false);
    }, this.reconnectIntervalSeconds() * 1000);
  }

  private reconnectIntervalSeconds(): number {
    return (
      this.view.settings?.values['websocket.reconnect_interval_sec'] ??
      DEFAULT_RECONNECT_INTERVAL_SECONDS
    );
  }

  private reconnectMaximum(): number {
    return (
      this.view.settings?.values['websocket.reconnect_max_retries'] ??
      DEFAULT_RECONNECT_MAX_RETRIES
    );
  }

  private heartbeatSeconds(): number {
    const values = this.view.settings?.values;
    return (
      (values?.['websocket.ping_interval_sec'] ?? DEFAULT_PING_INTERVAL_SECONDS) +
      (values?.['websocket.pong_timeout_sec'] ?? DEFAULT_PONG_TIMEOUT_SECONDS)
    );
  }

  private scheduleHeartbeat(): void {
    this.clearHeartbeatTimer();
    if (this.socket?.readyState !== SOCKET_OPEN) {
      return;
    }
    const deadline = this.lastPingAt + this.heartbeatSeconds() * 1000;
    const delay = Math.max(0, deadline - this.dependencies.now());
    this.heartbeatTimer = this.dependencies.setTimeout(() => {
      this.heartbeatTimer = undefined;
      const socket = this.socket;
      if (socket?.readyState !== SOCKET_OPEN) {
        return;
      }
      if (this.dependencies.now() >= deadline) {
        socket.close(4000, 'heartbeat timeout');
      } else {
        this.scheduleHeartbeat();
      }
    }, delay);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== undefined) {
      this.dependencies.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = undefined;
    }
  }

  private isActiveOpenSocket(socket: WebSocketLike): boolean {
    return !this.stopped && this.socket === socket && socket.readyState === SOCKET_OPEN;
  }

  private clearHeartbeatTimer(): void {
    if (this.heartbeatTimer !== undefined) {
      this.dependencies.clearTimeout(this.heartbeatTimer);
      this.heartbeatTimer = undefined;
    }
  }
}
