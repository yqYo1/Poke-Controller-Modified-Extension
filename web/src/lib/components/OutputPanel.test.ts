import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import OutputPanel from './OutputPanel.svelte';

describe('OutputPanel', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('keeps a per-profile note behind the Note tab', async () => {
    render(OutputPanel, {
      label: 'Output #1',
      lines: [],
      onclear: () => undefined,
      storageKey: 'pokecon.output1',
      profile: 'default'
    });

    await fireEvent.click(screen.getByRole('tab', { name: 'Note' }));
    const note = screen.getByLabelText('Output #1 note');
    await fireEvent.input(note, { target: { value: 'route memo' } });

    expect(localStorage.getItem('pokecon.output1.note.default')).toBe('route memo');
  });

  it('shows the log tab by default', () => {
    render(OutputPanel, {
      label: 'Output #2',
      lines: [{ id: 1, level: 'warning', message: 'warm', route: 'websocket', target: 'log' }],
      onclear: () => undefined,
      storageKey: 'pokecon.output2',
      profile: 'speedrun'
    });

    expect(screen.getByRole('tab', { name: 'Log' }).getAttribute('aria-selected')).toBe('true');
    expect(screen.getByText('warm')).toBeTruthy();
  });
});
