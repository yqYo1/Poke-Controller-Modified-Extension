import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import Page from './+page.svelte';

describe('application shell', () => {
  it('exposes the six canonical main tabs and the right-side widgets', () => {
    render(Page);

    expect(screen.getByRole('heading', { name: 'Controller workspace' })).toBeTruthy();
    expect(screen.getAllByRole('tab')).toHaveLength(6);
    expect(screen.getByRole('tab', { name: 'カメラ' }).getAttribute('aria-selected')).toBe(
      'true'
    );
    expect(screen.getByRole('region', { name: 'Software controller' })).toBeTruthy();
    expect(screen.getByRole('region', { name: 'Output #1' })).toBeTruthy();
    expect(screen.getByRole('region', { name: 'Output #2' })).toBeTruthy();
  });

  it('supports keyboard navigation across the tab list', async () => {
    render(Page);
    const camera = screen.getByRole('tab', { name: 'カメラ' });

    await fireEvent.keyDown(camera, { key: 'ArrowRight' });

    expect(screen.getByRole('tab', { name: 'シリアル' }).getAttribute('aria-selected')).toBe(
      'true'
    );
    expect(screen.getByRole('tabpanel').getAttribute('aria-labelledby')).toBe('tab-serial');
  });
});
