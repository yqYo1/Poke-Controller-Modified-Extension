import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import Page from './+page.svelte';

describe('application shell', () => {
  it('exposes all six main tabs with one selected tab', () => {
    render(Page);

    expect(screen.getByRole('heading', { name: 'Controller workspace' })).toBeTruthy();
    expect(screen.getAllByRole('tab')).toHaveLength(6);
    expect(screen.getByRole('tab', { name: 'Controller' }).getAttribute('aria-selected')).toBe(
      'true'
    );
  });
});
