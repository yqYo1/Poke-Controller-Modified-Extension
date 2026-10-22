import { describe, expect, it } from 'vitest';

import {
  InputManager,
  type InputRoute,
  type RoutedMessageSubscriber,
  type RoutedMessageTransport
} from './input';
import type { ClientMessage, ServerMessage } from './wire';

class FakeTransport implements RoutedMessageTransport {
  readonly sent: { message: ClientMessage; route: InputRoute }[] = [];
  private readonly subscribers = new Set<RoutedMessageSubscriber>();

  onMessage(subscriber: RoutedMessageSubscriber): () => void {
    this.subscribers.add(subscriber);
    return () => {
      this.subscribers.delete(subscriber);
    };
  }

  send(route: InputRoute, message: ClientMessage): boolean {
    this.sent.push({ message, route });
    return true;
  }

  receive(route: InputRoute, message: ServerMessage): void {
    for (const subscriber of this.subscribers) {
      subscriber(route, message);
    }
  }
}

function generation(generationValue: string): ServerMessage {
  return { data: { generation: generationValue }, type: 'input.generation' };
}

function applied(generationValue: string, sequence = '0'): ServerMessage {
  return {
    data: { generation: generationValue, sequence },
    type: 'input.snapshot.applied'
  };
}

function messageSequence(message: ClientMessage): string {
  if ('sequence' in message.data && typeof message.data.sequence === 'string') {
    return message.data.sequence;
  }
  throw new Error('input message does not carry a sequence');
}

describe('InputManager generation handoff', () => {
  it('sends a complete sequence-zero snapshot before ordered increments', () => {
    const transport = new FakeTransport();
    const manager = new InputManager(transport);
    manager.start();

    transport.receive('websocket', generation('ws-1'));
    manager.setGamepadButton('A', true);

    expect(transport.sent).toHaveLength(1);
    expect(transport.sent[0]).toMatchObject({
      route: 'websocket',
      message: {
        type: 'input.snapshot',
        data: {
          generation: 'ws-1',
          sequence: '0',
          buttons: { a: false },
          keyboard_keys: [],
          left_stick: { x: 128, y: 128 },
          right_stick: { x: 128, y: 128 },
          touch: null
        }
      }
    });

    transport.receive('websocket', applied('ws-1'));
    expect(transport.sent[1]).toEqual({
      route: 'websocket',
      message: {
        data: {
          button: 'A',
          generation: 'ws-1',
          kind: 'button',
          sequence: '1',
          state: 'pressed'
        },
        type: 'gamepad_input'
      }
    });

    manager.setGamepadHat('UP');
    expect(transport.sent[2]).toMatchObject({
      message: { data: { sequence: '2' }, type: 'gamepad_input' }
    });
  });

  it('preserves current state atomically when switching routes', () => {
    const transport = new FakeTransport();
    const manager = new InputManager(transport);
    manager.start();
    transport.receive('websocket', generation('ws-1'));
    transport.receive('websocket', applied('ws-1'));
    manager.setKeyboardKey('F5', true);
    manager.setGamepadStick('LSTICK', 255, 0);

    transport.receive('webrtc', generation('rtc-1'));
    const handoff = transport.sent.at(-1);
    expect(handoff).toMatchObject({
      route: 'webrtc',
      message: {
        type: 'input.snapshot',
        data: {
          generation: 'rtc-1',
          keyboard_keys: ['F5'],
          left_stick: { x: 255, y: 0 },
          sequence: '0'
        }
      }
    });

    transport.receive('websocket', applied('ws-1'));
    manager.setGamepadButton('B', true);
    expect(transport.sent.at(-1)).toBe(handoff);
    transport.receive('webrtc', applied('rtc-1'));
    expect(transport.sent.at(-1)).toMatchObject({
      route: 'webrtc',
      message: { data: { button: 'B', generation: 'rtc-1', sequence: '1' } }
    });
  });

  it('neutralizes every active incremental input before stopping', () => {
    const transport = new FakeTransport();
    const manager = new InputManager(transport);
    manager.start();
    transport.receive('websocket', generation('ws-1'));
    transport.receive('websocket', applied('ws-1'));
    manager.setKeyboardKey('Space', true);
    manager.setMouseButton('left', true, 10, 20);
    manager.setGamepadButton('ZR', true);
    manager.setGamepadButton('LCLICK', true);
    manager.setGamepadButton('RCLICK', true);
    manager.setGamepadHat('TOP_RIGHT');
    manager.setGamepadStick('RSTICK', 200, 40);
    manager.setGamepadTouch({ pressed: true, x: 319, y: 239 });
    const beforeStop = transport.sent.length;

    manager.stop();
    const releases = transport.sent.slice(beforeStop);
    expect(releases.map(({ message }) => message.type)).toEqual([
      'keyboard_input',
      'mouse_input',
      'gamepad_input',
      'gamepad_input',
      'gamepad_input',
      'gamepad_input',
      'gamepad_input',
      'gamepad_input'
    ]);
    expect(releases.map(({ message }) => messageSequence(message))).toEqual([
      '9',
      '10',
      '11',
      '12',
      '13',
      '14',
      '15',
      '16'
    ]);
  });

  it('rejects out-of-range values without mutating or sending', () => {
    const transport = new FakeTransport();
    const manager = new InputManager(transport);
    manager.start();

    expect(() => manager.setGamepadStick('LSTICK', 256, 0)).toThrow(RangeError);
    expect(() => manager.setGamepadTouch({ pressed: true, x: 320, y: 0 })).toThrow(RangeError);
    expect(transport.sent).toHaveLength(0);
  });

  it('clears local holds when the transport disconnects', () => {
    const transport = new FakeTransport();
    const manager = new InputManager(transport);
    let latestReady = false;
    let latestPressed = false;
    manager.subscribe((view) => {
      latestReady = view.ready;
      latestPressed = view.snapshot.buttons.a;
    });
    manager.start();
    transport.receive('websocket', generation('ws-1'));
    transport.receive('websocket', applied('ws-1'));
    manager.setGamepadButton('A', true);

    manager.transportDisconnected();
    expect(latestReady).toBe(false);
    expect(latestPressed).toBe(false);
  });
});
