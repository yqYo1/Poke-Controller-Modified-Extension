import { describe, expect, it } from 'vitest';

import {
  parseServerMessage,
  serializeClientMessage,
  validateWireValue,
  WireValidationError
} from './wire';

describe('generated wire schema validation', () => {
  it('accepts a closed server union variant', () => {
    expect(
      parseServerMessage(
        JSON.stringify({ type: 'ping', data: { nonce: 'heartbeat-1' } })
      )
    ).toEqual({ type: 'ping', data: { nonce: 'heartbeat-1' } });
  });

  it('rejects unknown fields and imprecise revisions', () => {
    expect(() =>
      parseServerMessage(
        JSON.stringify({ type: 'ping', data: { nonce: 'heartbeat-1' }, revision: '1' })
      )
    ).toThrow(WireValidationError);
    expect(() =>
      parseServerMessage(
        JSON.stringify({
          type: 'ui.state.changed',
          revision: 9_007_199_254_740_992,
          data: { cause: 'other', state: {}, settings: null }
        })
      )
    ).toThrow(WireValidationError);
  });

  it('validates nested ranges before sending client input', () => {
    expect(() =>
      serializeClientMessage({
        type: 'mouse_stick_input',
        data: {
          generation: 'generation-1',
          sequence: '1',
          stick: 'LSTICK',
          x: 256,
          y: 128
        }
      })
    ).toThrow(WireValidationError);
  });

  it('enforces generated collection constraints', () => {
    expect(() => validateWireValue('SettingsWriteValues', { 'ui.fps_options': [30, 30] })).toThrow(
      WireValidationError
    );
  });
});
