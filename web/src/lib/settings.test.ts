import { describe, expect, it, vi } from 'vitest';

import { ApiRequestError, type SettingsPatchRequest } from './api';
import { SettingsWriter, type SettingsGateway } from './settings';
import { settingsSnapshot } from './test-fixtures';
import type { SettingsSnapshot } from './wire';

function deferred<T>(): {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
} {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

describe('SettingsWriter', () => {
  it('serializes writes and advances expected revisions from committed snapshots', async () => {
    const first = deferred<SettingsSnapshot>();
    const patch = vi
      .fn<(request: SettingsPatchRequest) => Promise<SettingsSnapshot>>()
      .mockImplementationOnce(() => first.promise)
      .mockResolvedValueOnce(settingsSnapshot('12', { 'ui.fps': 60 }));
    const gateway: SettingsGateway = {
      load: vi.fn().mockResolvedValue(settingsSnapshot('10')),
      patch
    };
    const writer = new SettingsWriter(gateway);

    const firstWrite = writer.write({ 'ui.fps': 15 });
    const secondWrite = writer.write({ 'ui.fps': 60 });
    await vi.waitFor(() => expect(patch).toHaveBeenCalledTimes(1));
    expect(patch.mock.calls[0]?.[0]).toEqual({
      expected_revision: '10',
      values: { 'ui.fps': 15 }
    });

    first.resolve(settingsSnapshot('11', { 'ui.fps': 15 }));
    await expect(firstWrite).resolves.toMatchObject({ recoveredRevisionConflict: false });
    await expect(secondWrite).resolves.toMatchObject({ recoveredRevisionConflict: false });
    expect(patch.mock.calls[1]?.[0]).toEqual({
      expected_revision: '11',
      values: { 'ui.fps': 60 }
    });
  });

  it('refreshes and retries a revision conflict exactly once', async () => {
    const load = vi
      .fn<() => Promise<SettingsSnapshot>>()
      .mockResolvedValueOnce(settingsSnapshot('4'))
      .mockResolvedValueOnce(settingsSnapshot('6', { language: 'en' }));
    const patch = vi
      .fn<(request: SettingsPatchRequest) => Promise<SettingsSnapshot>>()
      .mockRejectedValueOnce(
        new ApiRequestError('stale revision', { code: 'revision_conflict', status: 409 })
      )
      .mockResolvedValueOnce(settingsSnapshot('7', { language: 'ja' }));
    const writer = new SettingsWriter({ load, patch });

    await expect(writer.write({ language: 'ja' })).resolves.toEqual({
      recoveredRevisionConflict: true,
      snapshot: settingsSnapshot('7', { language: 'ja' })
    });
    expect(load).toHaveBeenCalledTimes(2);
    expect(patch.mock.calls.map(([request]) => request.expected_revision)).toEqual(['4', '6']);
    expect(patch.mock.calls.map(([request]) => request.values)).toEqual([
      { language: 'ja' },
      { language: 'ja' }
    ]);
  });

  it('does not retry other failures and keeps the write queue usable', async () => {
    const failure = new ApiRequestError('persistence failed', {
      code: 'persistence_failed',
      status: 500
    });
    const patch = vi
      .fn<(request: SettingsPatchRequest) => Promise<SettingsSnapshot>>()
      .mockRejectedValueOnce(failure)
      .mockResolvedValueOnce(settingsSnapshot('2', { auto_reload_config: true }));
    const writer = new SettingsWriter({
      load: vi.fn().mockResolvedValue(settingsSnapshot('1')),
      patch
    });

    await expect(writer.write({ auto_reload_config: false })).rejects.toBe(failure);
    await expect(writer.write({ auto_reload_config: true })).resolves.toMatchObject({
      recoveredRevisionConflict: false
    });
    expect(patch).toHaveBeenCalledTimes(2);
  });

  it('publishes only snapshots at or after the current revision', () => {
    const writer = new SettingsWriter();
    const revisions: (string | null)[] = [];
    writer.subscribe((snapshot) => revisions.push(snapshot?.revision ?? null));

    writer.acceptSnapshot(settingsSnapshot('5'));
    writer.acceptSnapshot(settingsSnapshot('4'));
    writer.acceptSnapshot(settingsSnapshot('6'));

    expect(revisions).toEqual([null, '5', '6']);
  });
});
