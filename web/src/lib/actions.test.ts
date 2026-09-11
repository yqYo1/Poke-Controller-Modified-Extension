import { describe, expect, it, vi } from 'vitest';

import { BackendActions } from './actions';
import { ApiRequestError, createPokeconApi, REQUEST_MARKER } from './api';

function jsonResponse(value: unknown, init: ResponseInit = {}): Response {
  const headers = new Headers(init.headers);
  headers.set('Content-Type', 'application/json');
  return new Response(JSON.stringify(value), {
    headers,
    status: init.status ?? 200
  });
}

describe('BackendActions', () => {
  it('uses the generated client, fixed mutation marker, and exact request union', async () => {
    const request = vi.fn(async (input: Request) => {
      expect(input.url).toBe('https://pokecon.test/api/commands/control');
      expect(input.headers.get('X-Pokecon-Request')).toBe(REQUEST_MARKER);
      expect(await input.json()).toEqual({
        action: 'start',
        command: { class_name: 'MashA', module_path: 'Samples/MashA.py' }
      });
      return jsonResponse({ data: { changed: true, revision: '9' } });
    });
    const actions = new BackendActions(createPokeconApi(request, 'https://pokecon.test'));

    await expect(
      actions.controlCommand({
        action: 'start',
        command: { class_name: 'MashA', module_path: 'Samples/MashA.py' }
      })
    ).resolves.toEqual({ changed: true, revision: '9' });
    expect(request).toHaveBeenCalledOnce();
  });

  it('preserves structured error details from the common envelope', async () => {
    const actions = new BackendActions(
      createPokeconApi(
        () =>
          Promise.resolve(
            jsonResponse(
              {
                error: {
                  code: 'serial_connection_failed',
                  fields: { 'serial.port': ['configured port is unavailable'] },
                  message: 'serial connection failed'
                }
              },
              { status: 409 }
            )
          ),
        'https://pokecon.test'
      )
    );

    const error = await actions.controlSerial({ action: 'connect' }).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(ApiRequestError);
    expect(error).toMatchObject({
      code: 'serial_connection_failed',
      fields: { 'serial.port': ['configured port is unavailable'] },
      message: 'serial connection failed',
      status: 409
    });
  });

  it('returns binary downloads without attempting to parse them as JSON', async () => {
    const payload = new Uint8Array([0x89, 0x50, 0x4e, 0x47]);
    const actions = new BackendActions(
      createPokeconApi(
        async (input) => {
          expect(await input.clone().json()).toEqual({
            destination: 'download',
            filename: 'capture',
            format: 'png'
          });
          return new Response(payload, {
            headers: {
              'Content-Disposition': 'attachment; filename="capture.png"',
              'Content-Type': 'image/png'
            }
          });
        },
        'https://pokecon.test'
      )
    );

    const result = await actions.downloadScreenshot({
      destination: 'download',
      filename: 'capture',
      format: 'png'
    });

    expect(result.contentDisposition).toContain('capture.png');
    expect([...new Uint8Array(await result.blob.arrayBuffer())]).toEqual([...payload]);
  });
});
