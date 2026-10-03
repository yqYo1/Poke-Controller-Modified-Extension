import { describe, expect, it } from 'vitest';

import { backendTarget } from '../../vite.config';

describe('Vite backend target', () => {
  it('uses the canonical development defaults', () => {
    expect(backendTarget({})).toBe('http://127.0.0.1:8020');
  });

  it('formats configured IPv4 and IPv6 bind addresses', () => {
    expect(backendTarget({ POKECON_BIND_ADDRESS: '192.0.2.10', POKECON_PORT: '9000' })).toBe(
      'http://192.0.2.10:9000'
    );
    expect(backendTarget({ POKECON_BIND_ADDRESS: '::1', POKECON_PORT: '8021' })).toBe(
      'http://[::1]:8021'
    );
  });

  it('rejects an invalid port instead of silently targeting the wrong server', () => {
    expect(() => backendTarget({ POKECON_PORT: '0' })).toThrow(/POKECON_PORT/u);
    expect(() => backendTarget({ POKECON_PORT: '65536' })).toThrow(/POKECON_PORT/u);
  });
});
