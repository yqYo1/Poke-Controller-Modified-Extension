import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  chooseNativeSavePath,
  isDesktopShell,
  openNativeConfigDirectory
} from './desktop';

afterEach(() => {
  Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
});

describe('desktop bridge', () => {
  it('stays unavailable in a standalone browser', async () => {
    expect(isDesktopShell()).toBe(false);
    await expect(chooseNativeSavePath('capture.png', 'png')).rejects.toThrow('Web mode');
  });

  it('uses only the explicitly exposed Tauri commands', async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce('C:\\Captures\\capture.png')
      .mockResolvedValueOnce(undefined);
    Reflect.set(window, '__TAURI_INTERNALS__', { invoke });

    expect(isDesktopShell()).toBe(true);
    await expect(chooseNativeSavePath('capture.png', 'png')).resolves.toBe(
      'C:\\Captures\\capture.png'
    );
    await openNativeConfigDirectory();
    expect(invoke).toHaveBeenNthCalledWith(1, 'choose_save_path', {
      extension: 'png',
      suggestedName: 'capture.png'
    });
    expect(invoke).toHaveBeenNthCalledWith(2, 'open_config_directory');
  });
});
