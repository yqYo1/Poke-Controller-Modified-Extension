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
  tags: ['Rank'],
  usage_count: 5,
  modified_at: 100
};
const mcu: CommandInfo = {
  class_name: 'Mcu',
  module_path: 'Commands.PythonCommands.mcu',
  name: 'Mcu command',
  tags: ['Device']
};
const alpha: CommandInfo = {
  class_name: 'Alpha',
  module_path: 'Commands.PythonCommands.samples.alpha',
  name: 'Alpha',
  tags: ['Rank'],
  usage_count: 9,
  modified_at: 200
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
            { command: alpha, kind: 'command' },
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
    createScript: vi.fn(),
    previewScriptCreation: vi.fn(),
    reloadCommands: vi.fn().mockResolvedValue({ changed: true, revision: '2' }),
    runLists: vi.fn().mockResolvedValue({ lists: [], skipped: [] }),
    saveRunLists: vi.fn().mockResolvedValue({ lists: [], skipped: [] }),
    scriptCreatorSnapshot: vi.fn().mockResolvedValue({
      author: '',
      bridge_available: false,
      script_folders: [],
      template_folders: []
    })
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

  it('shows the command path in a right-click menu and copies it', async () => {
    const { runtime, view } = runtimeView();
    const actions = commandActions();
    render(CommandsTab, { actions, runtime, view });

    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    const row = await screen.findByRole('button', { name: 'Select First' });
    await fireEvent.contextMenu(row);

    expect(screen.getByRole('menu', { name: 'Actions for First' })).toBeTruthy();
    expect(screen.getByLabelText('Command path').textContent).toBe(
      'Commands.PythonCommands.samples.first'
    );

    await fireEvent.click(screen.getByRole('button', { name: 'Copy path' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('Commands.PythonCommands.samples.first'));
    expect(screen.getByText('Copied')).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('sorts commands by name and drops separators in sorted views', async () => {
    const { runtime, view } = runtimeView();
    const actions = commandActions();
    render(CommandsTab, { actions, runtime, view });

    // Default order keeps the backend display list.
    expect(await screen.findByRole('button', { name: 'Select First' })).toBeTruthy();

    const sort = screen.getByLabelText('Command order');
    await fireEvent.change(sort, { target: { value: 'name_asc' } });

    await waitFor(() => {
      const labels = screen
        .getAllByRole('button')
        .map((element) => element.getAttribute('aria-label'))
        .filter((label): label is string => (label?.startsWith('Select ') ?? false));
      expect(labels).toEqual(['Select Alpha', 'Select First']);
    });
    // The sorted view is commands-only: the separator label disappears.
    expect(screen.queryByText('Primary')).toBeNull();

    await fireEvent.change(sort, { target: { value: 'name_desc' } });
    await waitFor(() => {
      const descending = screen
        .getAllByRole('button')
        .map((element) => element.getAttribute('aria-label'))
        .filter((label): label is string => (label?.startsWith('Select ') ?? false));
      expect(descending).toEqual(['Select First', 'Select Alpha']);
    });
  });

  it('sorts commands by usage counts', async () => {
    const { runtime, view } = runtimeView();
    const actions = commandActions();
    render(CommandsTab, { actions, runtime, view });

    await screen.findByRole('button', { name: 'Select First' });
    const sort = screen.getByLabelText('Command order');
    await fireEvent.change(sort, { target: { value: 'usage_desc' } });
    await waitFor(() => {
      const labels = screen
        .getAllByRole('button')
        .map((element) => element.getAttribute('aria-label'))
        .filter((label): label is string => (label?.startsWith('Select ') ?? false));
      expect(labels).toEqual(['Select Alpha', 'Select First']);
    });

    await fireEvent.change(sort, { target: { value: 'usage_asc' } });
    await waitFor(() => {
      const labels = screen
        .getAllByRole('button')
        .map((element) => element.getAttribute('aria-label'))
        .filter((label): label is string => (label?.startsWith('Select ') ?? false));
      expect(labels).toEqual(['Select First', 'Select Alpha']);
    });
  });

  it('sorts commands by modification time', async () => {
    const { runtime, view } = runtimeView();
    const actions = commandActions();
    render(CommandsTab, { actions, runtime, view });

    await screen.findByRole('button', { name: 'Select First' });
    const sort = screen.getByLabelText('Command order');
    await fireEvent.change(sort, { target: { value: 'mtime_desc' } });
    await waitFor(() => {
      const labels = screen
        .getAllByRole('button')
        .map((element) => element.getAttribute('aria-label'))
        .filter((label): label is string => (label?.startsWith('Select ') ?? false));
      expect(labels).toEqual(['Select Alpha', 'Select First']);
    });

    await fireEvent.change(sort, { target: { value: 'mtime_asc' } });
    await waitFor(() => {
      const labels = screen
        .getAllByRole('button')
        .map((element) => element.getAttribute('aria-label'))
        .filter((label): label is string => (label?.startsWith('Select ') ?? false));
      expect(labels).toEqual(['Select First', 'Select Alpha']);
    });
  });

  it('opens the run-list editor from the toolbar', async () => {
    const { runtime, view } = runtimeView();
    const actions = commandActions();
    render(CommandsTab, { actions, runtime, view });

    await fireEvent.click(screen.getByRole('button', { name: 'Run lists' }));
    await waitFor(() => expect(actions.runLists).toHaveBeenCalled());
    expect(await screen.findByRole('heading', { name: 'Run list editor' })).toBeTruthy();
  });
});
