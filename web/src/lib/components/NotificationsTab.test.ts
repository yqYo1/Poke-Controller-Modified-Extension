import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import { settingsSnapshot } from '../test-fixtures';
import NotificationsTab from './NotificationsTab.svelte';

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
    view: { ...initial, settings: settingsSnapshot('1') }
  };
}

describe('NotificationsTab', () => {
  it('writes canonical Windows and Discord settings without exposing LINE UI', async () => {
    const { runtime, view } = runtimeView();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2')
    });
    const actions = {
      testNotification: vi.fn().mockResolvedValue({ delivered: true })
    };
    render(NotificationsTab, { actions, runtime, view });

    const windows = screen.getByRole('region', { name: 'Windows' });
    await fireEvent.click(within(windows).getByLabelText('スクリプト開始時に通知'));
    await fireEvent.change(screen.getByLabelText('ユーザー名'), {
      target: { value: 'PokeCon' }
    });

    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({
        'notifications.windows.on_script_start': true
      });
      expect(writeSettings).toHaveBeenCalledWith({
        'notifications.discord.username': 'PokeCon'
      });
    });
    expect(screen.queryByText(/LINE/)).toBeNull();
    expect(screen.getByLabelText('Webhook URL').getAttribute('type')).toBe('password');
  });

  it('sends typed test requests and reports delivery', async () => {
    const { runtime, view } = runtimeView();
    const actions = {
      testNotification: vi.fn().mockResolvedValue({ delivered: true })
    };
    render(NotificationsTab, { actions, runtime, view });

    await fireEvent.click(screen.getByRole('button', { name: 'Discord をテスト' }));

    await waitFor(() => {
      expect(actions.testNotification).toHaveBeenCalledWith({ channel: 'discord' });
      expect(screen.getByRole('status').textContent).toContain('テスト通知を送信しました');
    });
  });
});
