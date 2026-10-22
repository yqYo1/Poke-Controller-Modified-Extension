import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import { settingsSnapshot, stateSnapshot } from '../test-fixtures';
import ProfileStartupDialog from './ProfileStartupDialog.svelte';

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

describe('ProfileStartupDialog', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it('switches to the selected profile at startup', async () => {
    const { runtime, view } = runtimeView();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', { active_profile: 'speedrun' })
    });
    render(ProfileStartupDialog, { runtime, view });

    const select = await screen.findByLabelText('起動時プロファイル');
    await fireEvent.change(select, { target: { value: 'speedrun' } });
    await fireEvent.click(screen.getByRole('button', { name: '切替' }));

    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ active_profile: 'speedrun' });
    });
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
  });

  it('remembers an explicit dismissal', async () => {
    const { runtime, view } = runtimeView();
    render(ProfileStartupDialog, { runtime, view });

    await fireEvent.click(await screen.findByRole('button', { name: '現在のまま開始' }));
    expect(window.localStorage.getItem('pokecon.startup-profile-dismissed')).toBe('1');
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
  });
});
