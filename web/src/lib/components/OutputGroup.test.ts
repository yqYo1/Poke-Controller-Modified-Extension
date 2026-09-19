import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import { ApplicationRuntime, type RuntimeView } from '../runtime';
import OutputGroup from './OutputGroup.svelte';

function idleView(): { runtime: ApplicationRuntime; view: RuntimeView } {
  const runtime = new ApplicationRuntime();
  let initial: RuntimeView | undefined;
  const unsubscribe = runtime.subscribe((view) => {
    initial = view;
  });
  unsubscribe();
  if (initial === undefined) throw new Error('runtime did not publish its initial view');
  return { runtime, view: initial };
}

describe('OutputGroup', () => {
  it('stacks both outputs vertically and keeps the split ratio on rows', () => {
    const { runtime, view } = idleView();
    const rendered = render(OutputGroup, {
      runtime,
      showOutput1: true,
      showOutput2: true,
      splitRatio: 20,
      view
    });

    expect(screen.getByRole('region', { name: 'Output #1' })).toBeTruthy();
    expect(screen.getByRole('region', { name: 'Output #2' })).toBeTruthy();
    const stack = rendered.container.firstElementChild as HTMLElement | null;
    expect(stack?.className).toContain('grid-cols-1');
    expect(stack?.className).not.toContain('xl:grid-cols');
    const style = stack?.getAttribute('style') ?? '';
    expect(style).toContain('grid-template-rows');
    expect(style).toContain('minmax(8rem, 26fr) minmax(8rem, 74fr)');
    expect(style).toContain('--output-one: 26%');
  });

  it('renders a single output without the two-row split', () => {
    const { runtime, view } = idleView();
    render(OutputGroup, {
      runtime,
      showOutput1: true,
      showOutput2: false,
      splitRatio: 20,
      view
    });

    expect(screen.getByRole('region', { name: 'Output #1' })).toBeTruthy();
    expect(screen.queryByRole('region', { name: 'Output #2' })).toBeNull();
  });
});
