import type { components } from './generated/api';
import type { RealtimeClient } from './realtime';
import { incrementRevision, type DecimalString } from './revision';
import type { ClientMessage, ServerMessage } from './wire';

type ButtonState = components['schemas']['ButtonState'];
type GamepadButton = components['schemas']['GamepadButton'];
type GamepadHat = components['schemas']['GamepadHat'];
type Hat = components['schemas']['Hat'];
type InputSnapshot = components['schemas']['InputSnapshot'];
type MouseButton = components['schemas']['MouseButton'];
type PressState = components['schemas']['PressState'];
type StickName = components['schemas']['StickName'];
type StickPosition = components['schemas']['StickPosition'];
type TouchPoint = components['schemas']['TouchPoint'];

export type InputRoute = 'websocket' | 'webrtc';

export interface RoutedMessageTransport {
  onMessage(subscriber: RoutedMessageSubscriber): () => void;
  send(route: InputRoute, message: ClientMessage): boolean;
}

export type RoutedMessageSubscriber = (route: InputRoute, message: ServerMessage) => void;

export interface InputView {
  readonly generation: string | null;
  readonly ready: boolean;
  readonly route: InputRoute | null;
  readonly snapshot: InputState;
}

export type InputState = Omit<InputSnapshot, 'generation' | 'sequence'>;

type InputSubscriber = (view: InputView) => void;
type MessageBuilder = (generation: string, sequence: DecimalString) => ClientMessage;

const BUTTON_FIELDS: Readonly<Record<GamepadButton, keyof ButtonState>> = {
  A: 'a',
  B: 'b',
  CAPTURE: 'capture',
  HOME: 'home',
  L: 'l',
  MINUS: 'minus',
  PLUS: 'plus',
  R: 'r',
  X: 'x',
  Y: 'y',
  ZL: 'zl',
  ZR: 'zr'
};

const GAMEPAD_HATS: Readonly<Record<GamepadHat, Hat>> = {
  BTM_LEFT: 'down_left',
  BTM_RIGHT: 'down_right',
  CENTER: 'neutral',
  DOWN: 'down',
  LEFT: 'left',
  RIGHT: 'right',
  TOP_LEFT: 'up_left',
  TOP_RIGHT: 'up_right',
  UP: 'up'
};

const NEUTRAL_STICK: StickPosition = { x: 128, y: 128 };

function neutralButtons(): ButtonState {
  return {
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
  };
}

export function neutralInputState(): InputState {
  return {
    buttons: neutralButtons(),
    hat: 'neutral',
    keyboard_keys: [],
    left_stick: { ...NEUTRAL_STICK },
    mouse_buttons: { left: false, middle: false, right: false },
    right_stick: { ...NEUTRAL_STICK },
    touch: null
  };
}

function pressState(pressed: boolean): PressState {
  return pressed ? 'pressed' : 'released';
}

function assertCoordinate(value: number, maximum: number, name: string): void {
  if (!Number.isInteger(value) || value < 0 || value > maximum) {
    throw new RangeError(`${name} must be an integer from 0 through ${String(maximum)}`);
  }
}

function sameTouch(left: TouchPoint | null, right: TouchPoint | null): boolean {
  return (
    left === right ||
    (left !== null &&
      right !== null &&
      left.x === right.x &&
      left.y === right.y)
  );
}

function copyInputState(state: InputState): InputState {
  return {
    buttons: { ...state.buttons },
    hat: state.hat,
    keyboard_keys: [...state.keyboard_keys],
    left_stick: { ...state.left_stick },
    mouse_buttons: { ...state.mouse_buttons },
    right_stick: { ...state.right_stick },
    touch: state.touch === null ? null : { ...state.touch }
  };
}

export class WebSocketMessageTransport implements RoutedMessageTransport {
  constructor(private readonly client: RealtimeClient) {}

  onMessage(subscriber: RoutedMessageSubscriber): () => void {
    return this.client.onMessage((message) => {
      subscriber('websocket', message);
    });
  }

  send(route: InputRoute, message: ClientMessage): boolean {
    return route === 'websocket' && this.client.send(message);
  }
}

export class InputManager {
  private generation: string | null = null;
  private nextSequence: DecimalString = '1';
  private pendingMessages: MessageBuilder[] = [];
  private ready = false;
  private route: InputRoute | null = null;
  private snapshot: InputState = neutralInputState();
  private readonly subscribers = new Set<InputSubscriber>();
  private unsubscribe: (() => void) | undefined;

  constructor(private readonly transport: RoutedMessageTransport) {}

  subscribe(subscriber: InputSubscriber): () => void {
    this.subscribers.add(subscriber);
    subscriber(this.view());
    return () => {
      this.subscribers.delete(subscriber);
    };
  }

  start(): void {
    if (this.unsubscribe !== undefined) {
      return;
    }
    this.unsubscribe = this.transport.onMessage((route, message) => {
      this.handleMessage(route, message);
    });
  }

  stop(): void {
    this.neutralize();
    this.unsubscribe?.();
    this.unsubscribe = undefined;
    this.resetRoute();
  }

  resetRoute(): void {
    this.generation = null;
    this.nextSequence = '1';
    this.pendingMessages = [];
    this.ready = false;
    this.route = null;
    this.publish();
  }

  transportDisconnected(): void {
    this.snapshot = neutralInputState();
    this.resetRoute();
  }

  setKeyboardKey(key: string, pressed: boolean): void {
    const keys = new Set(this.snapshot.keyboard_keys);
    if (keys.has(key) === pressed) {
      return;
    }
    if (pressed) {
      keys.add(key);
    } else {
      keys.delete(key);
    }
    this.snapshot = { ...this.snapshot, keyboard_keys: [...keys].sort() };
    this.dispatch((generation, sequence) => ({
      data: { generation, key, sequence, state: pressState(pressed) },
      type: 'keyboard_input'
    }));
  }

  setMouseButton(
    button: MouseButton,
    pressed: boolean,
    x: number,
    y: number
  ): void {
    assertCoordinate(x, 4_294_967_295, 'mouse x');
    assertCoordinate(y, 4_294_967_295, 'mouse y');
    if (this.snapshot.mouse_buttons[button] === pressed) {
      return;
    }
    this.snapshot = {
      ...this.snapshot,
      mouse_buttons: { ...this.snapshot.mouse_buttons, [button]: pressed }
    };
    this.dispatch((generation, sequence) => ({
      data: { button, generation, sequence, state: pressState(pressed), x, y },
      type: 'mouse_input'
    }));
  }

  setMouseStick(stick: StickName, x: number, y: number): void {
    this.setStick(stick, x, y, (generation, sequence) => ({
      data: { generation, sequence, stick, x, y },
      type: 'mouse_stick_input'
    }));
  }

  setGamepadButton(button: GamepadButton, pressed: boolean): void {
    const field = BUTTON_FIELDS[button];
    if (this.snapshot.buttons[field] === pressed) {
      return;
    }
    this.snapshot = {
      ...this.snapshot,
      buttons: { ...this.snapshot.buttons, [field]: pressed }
    };
    this.dispatch((generation, sequence) => ({
      data: {
        button,
        generation,
        kind: 'button',
        sequence,
        state: pressState(pressed)
      },
      type: 'gamepad_input'
    }));
  }

  setGamepadStick(stick: StickName, x: number, y: number): void {
    this.setStick(stick, x, y, (generation, sequence) => ({
      data: { generation, kind: 'stick', sequence, stick, x, y },
      type: 'gamepad_input'
    }));
  }

  setGamepadHat(hat: GamepadHat): void {
    const canonicalHat = GAMEPAD_HATS[hat];
    if (this.snapshot.hat === canonicalHat) {
      return;
    }
    this.snapshot = { ...this.snapshot, hat: canonicalHat };
    this.dispatch((generation, sequence) => ({
      data: { generation, hat, kind: 'hat', sequence },
      type: 'gamepad_input'
    }));
  }

  setGamepadTouch(touch: TouchPoint | null): void {
    if (touch !== null) {
      assertCoordinate(touch.x, 319, 'touch x');
      assertCoordinate(touch.y, 239, 'touch y');
    }
    if (sameTouch(this.snapshot.touch, touch)) {
      return;
    }
    const nextTouch = touch === null ? null : { ...touch };
    this.snapshot = { ...this.snapshot, touch: nextTouch };
    this.dispatch((generation, sequence) => ({
      data: { generation, kind: 'touch', sequence, touch: nextTouch },
      type: 'gamepad_input'
    }));
  }

  neutralize(): void {
    for (const key of this.snapshot.keyboard_keys) {
      this.setKeyboardKey(key, false);
    }
    for (const button of ['left', 'right', 'middle'] as const) {
      if (this.snapshot.mouse_buttons[button]) {
        this.setMouseButton(button, false, 0, 0);
      }
    }
    for (const button of Object.keys(BUTTON_FIELDS) as GamepadButton[]) {
      if (this.snapshot.buttons[BUTTON_FIELDS[button]]) {
        this.setGamepadButton(button, false);
      }
    }
    if (this.snapshot.hat !== 'neutral') {
      this.setGamepadHat('CENTER');
    }
    if (
      this.snapshot.left_stick.x !== NEUTRAL_STICK.x ||
      this.snapshot.left_stick.y !== NEUTRAL_STICK.y
    ) {
      this.setGamepadStick('LSTICK', NEUTRAL_STICK.x, NEUTRAL_STICK.y);
    }
    if (
      this.snapshot.right_stick.x !== NEUTRAL_STICK.x ||
      this.snapshot.right_stick.y !== NEUTRAL_STICK.y
    ) {
      this.setGamepadStick('RSTICK', NEUTRAL_STICK.x, NEUTRAL_STICK.y);
    }
    if (this.snapshot.touch !== null) {
      this.setGamepadTouch(null);
    }
    this.snapshot = neutralInputState();
    this.publish();
  }

  private setStick(
    stick: StickName,
    x: number,
    y: number,
    builder: MessageBuilder
  ): void {
    assertCoordinate(x, 255, 'stick x');
    assertCoordinate(y, 255, 'stick y');
    const field = stick === 'LSTICK' ? 'left_stick' : 'right_stick';
    const current = this.snapshot[field];
    if (current.x === x && current.y === y) {
      return;
    }
    this.snapshot = { ...this.snapshot, [field]: { x, y } };
    this.dispatch(builder);
  }

  private handleMessage(route: InputRoute, message: ServerMessage): void {
    if (message.type === 'input.generation') {
      this.beginGeneration(route, message.data.generation);
    } else if (message.type === 'input.snapshot.applied') {
      this.applyAcknowledgement(route, message.data.generation, message.data.sequence);
    }
  }

  private beginGeneration(route: InputRoute, generation: string): void {
    this.generation = generation;
    this.nextSequence = '1';
    this.pendingMessages = [];
    this.ready = false;
    this.route = route;
    const snapshot: InputSnapshot = {
      ...copyInputState(this.snapshot),
      generation,
      sequence: '0'
    };
    this.transport.send(route, { data: snapshot, type: 'input.snapshot' });
    this.publish();
  }

  private applyAcknowledgement(
    route: InputRoute,
    generation: string,
    sequence: DecimalString
  ): void {
    if (
      route !== this.route ||
      generation !== this.generation ||
      sequence !== '0' ||
      this.ready
    ) {
      return;
    }
    this.ready = true;
    const pending = this.pendingMessages;
    this.pendingMessages = [];
    for (const builder of pending) {
      if (!this.sendBuilder(builder)) {
        break;
      }
    }
    this.publish();
  }

  private dispatch(builder: MessageBuilder): void {
    if (this.route === null || this.generation === null) {
      this.publish();
      return;
    }
    if (!this.ready) {
      this.pendingMessages.push(builder);
      this.publish();
      return;
    }
    this.sendBuilder(builder);
    this.publish();
  }

  private sendBuilder(builder: MessageBuilder): boolean {
    if (this.route === null || this.generation === null) {
      return false;
    }
    const sequence = this.nextSequence;
    const message = builder(this.generation, sequence);
    if (this.transport.send(this.route, message)) {
      this.nextSequence = incrementRevision(sequence);
      return true;
    }
    return false;
  }

  private view(): InputView {
    return {
      generation: this.generation,
      ready: this.ready,
      route: this.route,
      snapshot: copyInputState(this.snapshot)
    };
  }

  private publish(): void {
    const view = this.view();
    for (const subscriber of this.subscribers) {
      subscriber(view);
    }
  }
}
