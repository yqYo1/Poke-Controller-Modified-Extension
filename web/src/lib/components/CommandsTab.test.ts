import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { components } from '../api/openapi';
import { ApplicationRuntime, type RuntimeView } from '../runtime';
import { settingsSnapshot, stateSnapshot } from '../test-fixtures';
import CommandsTab from './CommandsTab.svelte';

type CommandInfo = components['schemas']['CommandInfo'];

const first: CommandInfo = {
  class_name: 'First',
  module_path: 'Commands.PythonCommands.samples.first',
  name: 'First',
  tags: ['Rank']
};
const mcu: CommandInfo = {
  class_name: 'Mcu',
  module_path: 'Commands.PythonCommands.mcu',
  name: 'Mcu command',
  tags: ['Device']
};

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
        command_candidates: [first, mcu],
        command_display_lists: {
          '-': [
            { kind: 'separator', label: 'Primary' },
            { command: first, kind: 'command' },
            { command: mcu, kind: 'command' }
          ],
          Rank: [{ command: first, kind: 'command' }]
        },
        tags: ['Rank', '@Samples']
      })
    }
  };
}

function commandActions() {
  return {
    controlCommand: vi.fn().mockResolvedValue({ changed: true, revision: '2' }),
    reloadCommands: vi.fn().mockResolvedValue({ changed: true, revision: '2' })
  };
}

describe('CommandsTab', () => {
  it('selects and starts a typed command identity', async () => {
    const { runtime, view } = runtimeView();
    const actions = commandActions();
    render(CommandsTab, { actions, runtime, view });

    await fireEvent.click(screen.getByRole('button', { name: 'Select First' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Start' }));

    await waitFor(() => {
      expect(actions.controlCommand).toHaveBeenCalledWith({
        action: 'start',
        command: {
          class_name: 'First',
          module_path: 'Commands.PythonCommands.samples.first'
        }
      });
    });
  });

  it('writes tag matching and persists shortcut module paths', async () => {
    const { runtime, view } = runtimeView();
    const actions = commandActions();
    const writeSettings = vi.spyOn(runtime, 'writeSettings').mockResolvedValue({
      recoveredRevisionConflict: false,
      snapshot: settingsSnapshot('2', {
        'commands.tag_match_mode': 'prefix',
        'shortcuts.button_1': first.module_path
      })
    });
    render(CommandsTab, { actions, runtime, view });

    await fireEvent.change(screen.getByLabelText('Tag match mode'), {
      target: { value: 'prefix' }
    });
    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({ 'commands.tag_match_mode': 'prefix' });
    });

    await fireEvent.click(screen.getByRole('tab', { name: 'Shortcut' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Shortcut 1: unassigned' }));
    await fireEvent.click(
      screen.getByRole('button', { name: 'Assign First to shortcut 1' })
    );
    await waitFor(() => {
      expect(writeSettings).toHaveBeenCalledWith({
        'shortcuts.button_1': first.module_path
      });
    });
  });

  it('uses fuzzy search results without backend separators', async () => {
    const { runtime, view } = runtimeView();
    render(CommandsTab, { actions: commandActions(), runtime, view });

    await fireEvent.input(screen.getByLabelText('Search commands'), {
      target: { value: 'fist' }
    });

    expect(screen.getByRole('button', { name: 'Select First' })).toBeTruthy();
    expect(screen.queryByRole('separator')).toBeNull();
  });
});
