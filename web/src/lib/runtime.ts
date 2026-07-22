import type { SettingsWriteValues } from './api';
import type { components } from './generated/api';
import {
  InputManager,
  type InputRoute,
  type InputView,
  type RoutedMessageTransport
} from './input';
import { MediaTransport, type MediaView } from './media';
import { RealtimeClient, type RealtimeView } from './realtime';
import { SettingsWriter, type SettingsWriteResult } from './settings';
import type { ServerMessage, SettingsSnapshot, StateSnapshot } from './wire';

type GamepadButton = components['schemas']['GamepadButton'];
type GamepadHat = components['schemas']['GamepadHat'];
type LogLevel = components['schemas']['LogLevel'];
type LogTarget = components['schemas']['LogTarget'];
type StickName = components['schemas']['StickName'];
type TouchPoint = components['schemas']['TouchPoint'];

const MAX_OUTPUT_LINES = 2_000;
const MAX_SERIAL_LINES = 1_000;

export interface OutputLine {
  readonly id: number;
  readonly level: LogLevel;
  readonly message: string;
  readonly route: InputRoute;
  readonly target: LogTarget;
}

export interface SerialLine {
  readonly base64: string;
  readonly byteLength: number;
  readonly id: number;
  readonly text: string;
}

export interface RuntimeView {
  readonly actionError: string | null;
  readonly input: InputView;
  readonly media: MediaView;
  readonly notice: string | null;
  readonly output1: readonly OutputLine[];
  readonly output2: readonly OutputLine[];
  readonly realtime: RealtimeView;
  readonly serial: readonly SerialLine[];
  readonly settings: SettingsSnapshot | null;
  readonly state: StateSnapshot | null;
}

interface RealtimeRuntime {
  reconnectNow(): void;
  start(): void;
  stop(): void;
  subscribe(subscriber: (view: RealtimeView) => void): () => void;
}

interface MediaRuntime extends RoutedMessageTransport {
  markVideoActivity(): void;
  onFallbackFrame(subscriber: (frame: ArrayBuffer | Blob) => void): () => void;
  reconnectWebRtc(): void;
  start(): void;
  stop(): void;
  subscribe(subscriber: (view: MediaView) => void): () => void;
}

interface InputRuntime {
  neutralize(): void;
  setGamepadButton(button: GamepadButton, pressed: boolean): void;
  setGamepadHat(hat: GamepadHat): void;
  setGamepadStick(stick: StickName, x: number, y: number): void;
  setGamepadTouch(touch: TouchPoint | null): void;
  setKeyboardKey(key: string, pressed: boolean): void;
  start(): void;
  stop(): void;
  subscribe(subscriber: (view: InputView) => void): () => void;
  transportDisconnected(): void;
}

interface SettingsRuntime {
  acceptSnapshot(snapshot: SettingsSnapshot): void;
  subscribe(subscriber: (snapshot: SettingsSnapshot | null) => void): () => void;
  write(values: SettingsWriteValues): Promise<SettingsWriteResult>;
}

export interface RuntimeBundle {
  readonly input: InputRuntime;
  readonly media: MediaRuntime;
  readonly realtime: RealtimeRuntime;
  readonly settings: SettingsRuntime;
}

type RuntimeSubscriber = (view: RuntimeView) => void;
type FallbackFrameSubscriber = (frame: ArrayBuffer | Blob) => void;

const idleRealtimeView: RealtimeView = {
  attempts: 0,
  lastError: null,
  latestRevision: null,
  maxRetries: 20,
  settings: null,
  state: null,
  status: 'idle'
};

const idleMediaView: MediaView = {
  lastError: null,
  mode: 'idle',
  negotiating: false,
  stream: null
};

const idleInputView: InputView = {
  generation: null,
  ready: false,
  route: null,
  snapshot: {
    buttons: {
      a: false,
      b: false,
      capture: false,
      home: false,
      l: false,
      lclick: false,
      minus: false,
      plus: false,
      r: false,
      rclick: false,
      x: false,
      y: false,
      zl: false,
      zr: false
    },
    hat: 'neutral',
    keyboard_keys: [],
    left_stick: { x: 128, y: 128 },
    mouse_buttons: { left: false, middle: false, right: false },
    right_stick: { x: 128, y: 128 },
    touch: null
  }
};

function createDefaultBundle(): RuntimeBundle {
  const realtime = new RealtimeClient();
  const media = MediaTransport.fromRealtimeClient(realtime);
  return {
    input: new InputManager(media),
    media,
    realtime,
    settings: new SettingsWriter()
  };
}

function boundedAppend<T>(items: readonly T[], item: T, maximum: number): readonly T[] {
  const start = Math.max(0, items.length + 1 - maximum);
  return [...items.slice(start), item];
}

function decodeSerialData(base64: string, byteLength: number): string {
  const binary = atob(base64);
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  if (bytes.byteLength !== byteLength) {
    throw new Error('serial payload byte length does not match its envelope');
  }
  return new TextDecoder().decode(bytes);
}

function safeMessage(error: unknown): string {
  return (error instanceof Error ? error.message : 'operation failed').slice(0, 512);
}

export class ApplicationRuntime {
  private readonly bundle: RuntimeBundle;
  private fallbackFrameSubscribers = new Set<FallbackFrameSubscriber>();
  private lineId = 0;
  private previousConnectionStatus: RealtimeView['status'] = 'idle';
  private started = false;
  private readonly subscribers = new Set<RuntimeSubscriber>();
  private unsubscribers: (() => void)[] = [];
  private view: RuntimeView = {
    actionError: null,
    input: idleInputView,
    media: idleMediaView,
    notice: null,
    output1: [],
    output2: [],
    realtime: idleRealtimeView,
    serial: [],
    settings: null,
    state: null
  };

  constructor(bundle: RuntimeBundle = createDefaultBundle()) {
    this.bundle = bundle;
  }

  subscribe(subscriber: RuntimeSubscriber): () => void {
    this.subscribers.add(subscriber);
    subscriber(this.view);
    return () => {
      this.subscribers.delete(subscriber);
    };
  }

  onFallbackFrame(subscriber: FallbackFrameSubscriber): () => void {
    this.fallbackFrameSubscribers.add(subscriber);
    return () => {
      this.fallbackFrameSubscribers.delete(subscriber);
    };
  }

  start(): void {
    if (this.started) {
      return;
    }
    this.started = true;
    this.unsubscribers = [
      this.bundle.realtime.subscribe((view) => this.handleRealtimeView(view)),
      this.bundle.media.subscribe((view) => this.updateView({ media: view })),
      this.bundle.input.subscribe((view) => this.updateView({ input: view })),
      this.bundle.media.onMessage((route, message) => this.handleMessage(route, message)),
      this.bundle.media.onFallbackFrame((frame) => {
        for (const subscriber of this.fallbackFrameSubscribers) {
          subscriber(frame);
        }
      }),
      this.bundle.settings.subscribe((settings) => this.updateView({ settings }))
    ];

    this.bundle.input.start();
    this.bundle.media.start();
    this.bundle.realtime.start();
  }

  stop(): void {
    if (!this.started) {
      return;
    }
    this.bundle.input.stop();
    this.bundle.media.stop();
    this.bundle.realtime.stop();
    for (const unsubscribe of this.unsubscribers) {
      unsubscribe();
    }
    this.unsubscribers = [];
    this.started = false;
  }

  async writeSettings(values: SettingsWriteValues): Promise<SettingsWriteResult> {
    this.updateView({ actionError: null, notice: null });
    try {
      const result = await this.bundle.settings.write(values);
      const failures = Object.values(result.snapshot.apply_failures);
      this.updateView({
        actionError: failures.length === 0 ? null : failures.join('\n'),
        notice: result.recoveredRevisionConflict
          ? '設定の競合を最新状態へ再適用しました。'
          : '設定を保存しました。'
      });
      return result;
    } catch (error: unknown) {
      this.updateView({ actionError: safeMessage(error), notice: null });
      throw error;
    }
  }

  dismissFeedback(): void {
    this.updateView({ actionError: null, notice: null });
  }

  clearOutputs(): void {
    this.updateView({ output1: [], output2: [] });
  }

  clearOutput(output: 1 | 2): void {
    this.updateView(output === 1 ? { output1: [] } : { output2: [] });
  }

  clearSerial(): void {
    this.updateView({ serial: [] });
  }

  reconnectWebSocket(): void {
    this.bundle.realtime.reconnectNow();
  }

  reconnectWebRtc(): void {
    this.bundle.media.reconnectWebRtc();
  }

  markVideoActivity(): void {
    this.bundle.media.markVideoActivity();
  }

  setKeyboardKey(key: string, pressed: boolean): void {
    this.bundle.input.setKeyboardKey(key, pressed);
  }

  setGamepadButton(button: GamepadButton, pressed: boolean): void {
    this.bundle.input.setGamepadButton(button, pressed);
  }

  setGamepadHat(hat: GamepadHat): void {
    this.bundle.input.setGamepadHat(hat);
  }

  setGamepadStick(stick: StickName, x: number, y: number): void {
    this.bundle.input.setGamepadStick(stick, x, y);
  }

  setGamepadTouch(touch: TouchPoint | null): void {
    this.bundle.input.setGamepadTouch(touch);
  }

  neutralizeInput(): void {
    this.bundle.input.neutralize();
  }

  private handleRealtimeView(realtime: RealtimeView): void {
    const disconnected =
      this.previousConnectionStatus !== 'idle' &&
      (realtime.status === 'idle' ||
        realtime.status === 'waiting' ||
        realtime.status === 'exhausted');
    this.previousConnectionStatus = realtime.status;
    if (disconnected) {
      this.bundle.input.transportDisconnected();
    }
    if (realtime.settings !== null) {
      this.bundle.settings.acceptSnapshot(realtime.settings);
    }
    this.updateView({ realtime, state: realtime.state });
  }

  private handleMessage(route: InputRoute, message: ServerMessage): void {
    if (message.type === 'log') {
      this.appendOutput(route, message.data);
    } else if (message.type === 'serial.data') {
      try {
        const serial: SerialLine = {
          base64: message.data.data,
          byteLength: message.data.byte_length,
          id: this.nextLineId(),
          text: decodeSerialData(message.data.data, message.data.byte_length)
        };
        this.updateView({
          serial: boundedAppend(this.view.serial, serial, MAX_SERIAL_LINES)
        });
      } catch (error: unknown) {
        this.updateView({ actionError: safeMessage(error) });
      }
    }
  }

  private appendOutput(
    route: InputRoute,
    data: Extract<ServerMessage, { type: 'log' }>['data']
  ): void {
    const line: OutputLine = { ...data, id: this.nextLineId(), route };
    const stdoutDestination =
      this.view.settings?.values['ui.stdout_destination'] ?? 'output_1';
    const output =
      data.target === 'panel2' ||
      (data.target === 'stdout' && stdoutDestination === 'output_2')
        ? 'output2'
        : 'output1';
    this.updateView({
      [output]: boundedAppend(this.view[output], line, MAX_OUTPUT_LINES)
    });
  }

  private nextLineId(): number {
    this.lineId += 1;
    return this.lineId;
  }

  private updateView(update: Partial<RuntimeView>): void {
    this.view = { ...this.view, ...update };
    for (const subscriber of this.subscribers) {
      subscriber(this.view);
    }
  }
}
