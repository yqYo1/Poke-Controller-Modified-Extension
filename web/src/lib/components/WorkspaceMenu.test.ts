import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import { settingsSnapshot, stateSnapshot } from '../test-fixtures';
import WorkspaceMenu from './WorkspaceMenu.svelte';

afterEach(() => {
  Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
});

function runtimeView(): { runtime: ApplicationRuntime; view: RuntimeView } {
  const runtime = new ApplicationRuntime();
  let initial: RuntimeView | undefined;
  const unsubscribe = runtime.subscribe((view) => {
    initial = view;
  });
  unsubscribe();
  if (initial === undefined) throw new Error('runtime did not publish its initial view');
  return {
    runtime,
    view: {
      ...initial,
      settings: settingsSnapshot('1'),
      state: stateSnapshot('1', {
        active_profile: 'default',
        available_profiles: ['default', 'speedrun']
      })
    }
  };
}

function menuActions() {
  return {
    checkUpdate: vi.fn().mockResolvedValue({
      current_version: '0.1.0',
      latest_version: '0.2.0',
      release_url: 'https://example.test/release',
      update_available: true
    }),
    controlDynamicConfig: vi.fn().mockResolvedValue({
      display_path: '/config/init.py',
      language: 'python',
      loaded: true
    }),
    downloadLauncher: vi.fn().mockResolvedValue({
      blob: new Blob(['launcher']),
      contentDisposition: 'attachment; filename="profile.bat"'
    }),
    generateLauncher: vi.fn().mockResolvedValue({
      launcher_created: true,
      profile_created: false,
      revision: '2'
    })
  };
}

describe('WorkspaceMenu', () => {
  it('requires an explicit action before switching profiles', async () => {
    const { runtime, view } = runtimeView();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', { active_profile: 'speedrun' })
    });
    render(WorkspaceMenu, { actions: menuActions(), runtime, view });
    await fireEvent.click(screen.getByText('メニュー'));

    await fireEvent.change(screen.getByLabelText('選択プロファイル'), {
      target: { value: 'speedrun' }
    });
    expect(writeSettings).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: '切替' }));

    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ active_profile: 'speedrun' });
    });
  });

  it('honors dynamic-config cancellation without sending file content', async () => {
    const { runtime, view } = runtimeView();
    const actions = menuActions();
    const confirmOverwrite = vi.fn().mockReturnValue(false);
    const { container } = render(WorkspaceMenu, {
      actions,
      confirmOverwrite,
      runtime,
      view
    });
    const input = container.querySelector<HTMLInputElement>('input[type="file"]');
    if (input === null) throw new Error('dynamic file input was not rendered');
    const file = new File(['value = 1'], 'init.py', { type: 'text/x-python' });

    await fireEvent.change(input, { target: { files: [file] } });

    expect(confirmOverwrite).toHaveBeenCalledOnce();
    expect(actions.controlDynamicConfig).not.toHaveBeenCalled();
  });

  it('loads browser file content and exposes the resolved directory', async () => {
    const { runtime, view } = runtimeView();
    const actions = menuActions();
    const { container } = render(WorkspaceMenu, {
      actions,
      confirmOverwrite: () => true,
      runtime,
      view
    });
    const input = container.querySelector<HTMLInputElement>('input[type="file"]');
    if (input === null) throw new Error('dynamic file input was not rendered');
    const file = new File(['value = 1'], 'settings.py', { type: 'text/x-python' });

    await fireEvent.change(input, { target: { files: [file] } });

    await waitFor(() => {
      expect(actions.controlDynamicConfig).toHaveBeenCalledWith({
        action: 'load_content',
        content: 'value = 1',
        language: 'python'
      });
      expect(screen.getByText('/config')).toBeTruthy();
    });
  });

  it('disables Windows launcher generation on other platforms and checks updates', async () => {
    const { runtime, view } = runtimeView();
    const actions = menuActions();
    render(WorkspaceMenu, { actions, runtime, view, windows: false });

    await fireEvent.click(screen.getByText('メニュー'));
    expect(screen.getByRole('button', { name: '.bat をダウンロード' })).toHaveProperty(
      'disabled',
      true
    );
    await fireEvent.click(screen.getByText('ヘルプ'));
    await fireEvent.click(screen.getByRole('button', { name: 'アップデート確認' }));

    await waitFor(() => {
      expect(actions.checkUpdate).toHaveBeenCalledOnce();
      expect(screen.getByText('0.1.0 → 0.2.0')).toBeTruthy();
    });
  });

  it('opens native config and saves a launcher to a native desktop path', async () => {
    const invoke = vi.fn((command: string) =>
      Promise.resolve(
        command === 'choose_save_path' ? 'C:\\Launchers\\default.bat' : undefined
      )
    );
    Reflect.set(window, '__TAURI_INTERNALS__', { invoke });
    const { runtime, view } = runtimeView();
    const actions = menuActions();
    render(WorkspaceMenu, { actions, runtime, view, windows: true });

    await fireEvent.click(screen.getByText('メニュー'));
    await fireEvent.click(screen.getByRole('button', { name: 'ディレクトリを開く' }));
    await fireEvent.click(screen.getByRole('button', { name: '.bat を保存' }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('open_config_directory');
      expect(invoke).toHaveBeenCalledWith('choose_save_path', {
        extension: 'bat',
        suggestedName: 'default.bat'
      });
      expect(actions.generateLauncher).toHaveBeenCalledWith({
        copy_current: false,
        destination: { kind: 'path', path: 'C:\\Launchers\\default.bat' },
        profile: 'default'
      });
    });
    expect(actions.downloadLauncher).not.toHaveBeenCalled();
  });
});
